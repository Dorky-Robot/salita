use salita::db;
use tempfile::TempDir;

#[tokio::test]
async fn test_register_node_respects_provided_node_id() {
    // Setup: Create test database in a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db_pool = db::create_pool(&db_path).expect("Failed to create test database");
    db::run_migrations(&db_pool).expect("Failed to run migrations");

    // Test data - simulating a phone with persistent node ID
    let phone_node_id = uuid::Uuid::now_v7().to_string();
    let phone_hostname = "192.168.1.110";
    let phone_name = "Test Phone";

    // Register the device for the first time with explicit node_id
    let register_query = format!(
        r#"mutation {{
            registerNode(input: {{
                nodeId: "{}",
                name: "{}",
                hostname: "{}",
                port: 6969
            }}) {{
                success
                message
                node {{ id name hostname }}
            }}
        }}"#,
        phone_node_id, phone_name, phone_hostname
    );

    // Execute GraphQL mutation
    let schema = salita::graphql::build_schema();
    let request = async_graphql::Request::new(register_query.clone()).data(db_pool.clone());
    let result = schema.execute(request).await;

    // Assert: Registration succeeded
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );

    let data = result.data.into_json().unwrap();
    let register_result = &data["registerNode"];

    assert_eq!(
        register_result["success"].as_bool().unwrap(),
        true,
        "Registration should succeed"
    );

    // Critical assertion: The returned node ID should match the provided one
    let returned_node_id = register_result["node"]["id"].as_str().unwrap();
    assert_eq!(
        returned_node_id, phone_node_id,
        "Node ID should match the provided persistent ID, not generate a new one"
    );

    // Verify the hostname is correct
    assert_eq!(
        register_result["node"]["hostname"].as_str().unwrap(),
        phone_hostname
    );

    // Try to register the same device again with the same node_id (should update, not error)
    let request2 = async_graphql::Request::new(register_query).data(db_pool.clone());
    let result2 = schema.execute(request2).await;

    // This should either succeed (upsert) or give a helpful message
    let data2 = result2.data.into_json().unwrap();
    let _register_result2 = &data2["registerNode"];

    // Should not create a duplicate - check database
    let conn = db_pool.get().expect("Failed to get connection");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mesh_nodes WHERE hostname = ?",
            rusqlite::params![phone_hostname],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        count, 1,
        "Should only have one entry for this device, not multiple duplicates"
    );
}

#[tokio::test]
async fn test_register_node_without_node_id_generates_uuid() {
    // Setup
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test2.db");
    let db_pool = db::create_pool(&db_path).expect("Failed to create test database");
    db::run_migrations(&db_pool).expect("Failed to run migrations");

    // Register device without providing node_id (server should generate one)
    let register_query = r#"mutation {
        registerNode(input: {
            name: "Auto Node",
            hostname: "192.168.1.111",
            port: 6969
        }) {
            success
            message
            node { id name }
        }
    }"#;

    let schema = salita::graphql::build_schema();
    let request = async_graphql::Request::new(register_query).data(db_pool.clone());
    let result = schema.execute(request).await;

    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );

    let data = result.data.into_json().unwrap();
    let register_result = &data["registerNode"];

    assert_eq!(register_result["success"].as_bool().unwrap(), true);

    // Node ID should be a valid UUID (server-generated)
    let node_id = register_result["node"]["id"].as_str().unwrap();
    assert!(
        uuid::Uuid::parse_str(node_id).is_ok(),
        "Should have generated a valid UUID"
    );
}

#[tokio::test]
async fn test_duplicate_hostname_detection() {
    // Setup
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test3.db");
    let db_pool = db::create_pool(&db_path).expect("Failed to create test database");
    db::run_migrations(&db_pool).expect("Failed to run migrations");

    let hostname = "192.168.1.112";

    // Register first device
    let register_query1 = format!(
        r#"mutation {{
            registerNode(input: {{
                name: "Device 1",
                hostname: "{}",
                port: 6969
            }}) {{
                success
                message
                node {{ id }}
            }}
        }}"#,
        hostname
    );

    let schema = salita::graphql::build_schema();
    let request1 = async_graphql::Request::new(register_query1).data(db_pool.clone());
    let result1 = schema.execute(request1).await;

    assert!(result1.errors.is_empty());
    let data1 = result1.data.into_json().unwrap();
    assert_eq!(data1["registerNode"]["success"].as_bool().unwrap(), true);

    // Try to register different device with SAME hostname and DIFFERENT node_id
    let different_node_id = uuid::Uuid::now_v7().to_string();
    let register_query2 = format!(
        r#"mutation {{
            registerNode(input: {{
                nodeId: "{}",
                name: "Device 2",
                hostname: "{}",
                port: 6969
            }}) {{
                success
                message
            }}
        }}"#,
        different_node_id, hostname
    );

    let request2 = async_graphql::Request::new(register_query2).data(db_pool.clone());
    let result2 = schema.execute(request2).await;

    assert!(result2.errors.is_empty());
    let data2 = result2.data.into_json().unwrap();

    // Should fail with duplicate message
    assert_eq!(
        data2["registerNode"]["success"].as_bool().unwrap(),
        false,
        "Should reject duplicate hostname"
    );

    assert!(
        data2["registerNode"]["message"]
            .as_str()
            .unwrap()
            .contains("already connected"),
        "Error message should mention device is already connected"
    );
}
