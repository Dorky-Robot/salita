use crate::db::DbPool;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rusqlite::params;
use std::collections::HashMap;
use tokio::sync::watch;

const SERVICE_TYPE: &str = "_salita._tcp.local.";

pub struct MdnsDiscovery {
    daemon: ServiceDaemon,
    instance_fullname: String,
}

impl MdnsDiscovery {
    pub fn start(
        node_id: &str,
        node_name: &str,
        port: u16,
        pool: DbPool,
        shutdown_rx: watch::Receiver<bool>,
    ) -> anyhow::Result<Self> {
        let daemon = ServiceDaemon::new()?;

        let mut properties = HashMap::new();
        properties.insert("id".to_string(), node_id.to_string());
        properties.insert("name".to_string(), node_name.to_string());

        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "salita-node".to_string());
        let host_label = format!("{}.local.", hostname);

        let instance_name = node_name
            .replace('.', "-")
            .chars()
            .take(63)
            .collect::<String>();

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &host_label,
            "",
            port,
            properties,
        )?
        .enable_addr_auto();

        let instance_fullname = service_info.get_fullname().to_string();

        daemon.register(service_info)?;
        tracing::info!("mDNS: registered as {}", instance_fullname);

        let browse_receiver = daemon.browse(SERVICE_TYPE)?;
        let my_node_id = node_id.to_string();

        tokio::spawn(async move {
            Self::discovery_loop(browse_receiver, pool, my_node_id, shutdown_rx).await;
        });

        Ok(Self {
            daemon,
            instance_fullname,
        })
    }

    pub fn shutdown(self) {
        if let Err(e) = self.daemon.unregister(&self.instance_fullname) {
            tracing::warn!("mDNS: failed to unregister: {}", e);
        }
        if let Err(e) = self.daemon.shutdown() {
            tracing::warn!("mDNS: failed to shut down daemon: {}", e);
        }
    }

    async fn discovery_loop(
        receiver: flume::Receiver<ServiceEvent>,
        pool: DbPool,
        my_node_id: String,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::info!("mDNS: discovery loop shutting down");
                        break;
                    }
                }
                event = receiver.recv_async() => {
                    match event {
                        Ok(ServiceEvent::ServiceResolved(info)) => {
                            Self::handle_resolved(&pool, &info, &my_node_id);
                        }
                        Ok(ServiceEvent::ServiceRemoved(_ty, fullname)) => {
                            Self::handle_removed(&pool, &fullname);
                        }
                        Ok(_) => {}
                        Err(_) => {
                            tracing::debug!("mDNS: browse channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    fn handle_resolved(pool: &DbPool, info: &ServiceInfo, my_node_id: &str) {
        let peer_id = match info.get_property_val_str("id") {
            Some(id) => id.to_string(),
            None => return,
        };

        if peer_id == my_node_id {
            return;
        }

        let peer_name = info
            .get_property_val_str("name")
            .unwrap_or("Unknown")
            .to_string();

        let port = info.get_port();

        let endpoint = info
            .get_addresses_v4()
            .iter()
            .next()
            .map(|ip| ip.to_string())
            .or_else(|| {
                info.get_addresses()
                    .iter()
                    .next()
                    .map(|ip| ip.to_string())
            })
            .unwrap_or_default();

        if endpoint.is_empty() {
            return;
        }

        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("mDNS: db error: {}", e);
                return;
            }
        };

        let result = conn.execute(
            "INSERT INTO devices (id, name, endpoint, port, status, last_seen, is_self)
             VALUES (?1, ?2, ?3, ?4, 'online', datetime('now'), 0)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               endpoint = excluded.endpoint,
               port = excluded.port,
               status = 'online',
               last_seen = datetime('now')",
            params![peer_id, peer_name, endpoint, port],
        );

        match result {
            Ok(_) => tracing::info!(
                "mDNS: peer online — {} ({}) at {}:{}",
                peer_name,
                peer_id,
                endpoint,
                port
            ),
            Err(e) => tracing::warn!("mDNS: failed to upsert peer {}: {}", peer_id, e),
        }
    }

    fn handle_removed(pool: &DbPool, fullname: &str) {
        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("mDNS: db error: {}", e);
                return;
            }
        };

        let instance_name = fullname
            .strip_suffix(&format!(".{}", SERVICE_TYPE))
            .unwrap_or(fullname);

        let result = conn.execute(
            "UPDATE devices SET status = 'offline' WHERE name = ?1 AND is_self = 0",
            params![instance_name],
        );

        match result {
            Ok(changed) if changed > 0 => {
                tracing::info!("mDNS: peer offline — {}", instance_name);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("mDNS: failed to mark peer offline: {}", e);
            }
        }
    }
}
