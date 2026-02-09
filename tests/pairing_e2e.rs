use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// E2E test for the complete pairing flow
/// Tests the critical user journey: Desktop generates QR → Mobile scans → PIN entry → Auto-redirect
#[tokio::test]
async fn test_complete_pairing_flow() {
    // Start test server (this would need to be set up)
    let base_url = "http://localhost:6968";
    let client = Client::new();

    // STEP 1: Desktop opens join modal and gets token
    let modal_response = client
        .get(format!("{}/mesh/join-modal", base_url))
        .send()
        .await
        .expect("Failed to fetch join modal");

    assert!(modal_response.status().is_success());
    let modal_html = modal_response.text().await.unwrap();

    // Extract join URL from modal HTML
    let join_url = extract_join_url(&modal_html).expect("Failed to extract join URL");
    let token = extract_token_from_url(&join_url).expect("Failed to extract token");

    // STEP 2: Mobile device scans QR and accesses join page
    let join_response = client
        .get(&join_url)
        .send()
        .await
        .expect("Failed to access join page");

    assert!(join_response.status().is_success());
    let join_html = join_response.text().await.unwrap();

    // Extract PIN from join page
    let pin = extract_pin_from_html(&join_html).expect("Failed to extract PIN");
    assert_eq!(pin.len(), 6, "PIN should be 6 digits");
    assert!(pin.chars().all(|c| c.is_ascii_digit()), "PIN should be all digits");

    // STEP 3: Desktop user enters PIN and registers the device
    let verify_response = client
        .post(format!("{}/mesh/verify-join-pin", base_url))
        .json(&json!({
            "token": token,
            "pin": pin
        }))
        .send()
        .await
        .expect("Failed to verify PIN");

    assert!(verify_response.status().is_success());

    // Register the node via GraphQL
    let register_response = client
        .post(format!("{}/graphql", base_url))
        .json(&json!({
            "query": r#"
                mutation RegisterNode($input: RegisterNodeInput!) {
                    registerNode(input: $input) {
                        success
                        message
                    }
                }
            "#,
            "variables": {
                "input": {
                    "name": "Test Phone",
                    "hostname": "192.168.1.100",
                    "port": 6969
                }
            }
        }))
        .send()
        .await
        .expect("Failed to register node");

    assert!(register_response.status().is_success());
    let register_result: serde_json::Value = register_response.json().await.unwrap();
    assert_eq!(
        register_result["data"]["registerNode"]["success"],
        true,
        "Node registration should succeed"
    );

    // STEP 4: Mobile polls GraphQL and detects it's been added
    let mut attempts = 0;
    let max_attempts = 10;
    let mut found_in_mesh = false;

    while attempts < max_attempts {
        let nodes_response = client
            .post(format!("{}/graphql", base_url))
            .json(&json!({
                "query": "{ nodes { id name } }"
            }))
            .send()
            .await
            .expect("Failed to query nodes");

        if nodes_response.status().is_success() {
            let nodes_result: serde_json::Value = nodes_response.json().await.unwrap();
            let node_count = nodes_result["data"]["nodes"]
                .as_array()
                .map(|arr| arr.len())
                .unwrap_or(0);

            if node_count > 1 {
                found_in_mesh = true;
                break;
            }
        }

        attempts += 1;
        sleep(Duration::from_millis(500)).await;
    }

    assert!(
        found_in_mesh,
        "Mobile should detect it's been added to mesh within {} attempts",
        max_attempts
    );
}

/// Test that join tokens expire after TTL
#[tokio::test]
async fn test_join_token_expiry() {
    let base_url = "http://localhost:6968";
    let client = Client::new();

    // Get a join token
    let modal_response = client
        .get(format!("{}/mesh/join-modal", base_url))
        .send()
        .await
        .expect("Failed to fetch join modal");

    let modal_html = modal_response.text().await.unwrap();
    let join_url = extract_join_url(&modal_html).expect("Failed to extract join URL");

    // Token should work immediately
    let immediate_response = client.get(&join_url).send().await.unwrap();
    assert!(immediate_response.status().is_success());

    // Wait for token to expire (5 minutes + buffer)
    // In a real test, you'd mock the clock or use a shorter TTL for testing
    sleep(Duration::from_secs(301)).await;

    // Token should be expired
    let expired_response = client.get(&join_url).send().await.unwrap();
    assert!(
        expired_response.status().is_client_error(),
        "Expired token should return error"
    );
}

/// Test that PINs are single-use
#[tokio::test]
async fn test_pin_single_use() {
    let base_url = "http://localhost:6968";
    let client = Client::new();

    // Setup: Get token and PIN
    let modal_response = client
        .get(format!("{}/mesh/join-modal", base_url))
        .send()
        .await
        .unwrap();

    let modal_html = modal_response.text().await.unwrap();
    let join_url = extract_join_url(&modal_html).unwrap();
    let token = extract_token_from_url(&join_url).unwrap();

    let join_response = client.get(&join_url).send().await.unwrap();
    let join_html = join_response.text().await.unwrap();
    let pin = extract_pin_from_html(&join_html).unwrap();

    // First PIN verification should succeed
    let first_verify = client
        .post(format!("{}/mesh/verify-join-pin", base_url))
        .json(&json!({
            "token": token,
            "pin": pin
        }))
        .send()
        .await
        .unwrap();

    assert!(first_verify.status().is_success());

    // Second PIN verification with same token should fail
    let second_verify = client
        .post(format!("{}/mesh/verify-join-pin", base_url))
        .json(&json!({
            "token": token,
            "pin": pin
        }))
        .send()
        .await
        .unwrap();

    assert!(
        second_verify.status().is_client_error(),
        "PIN should only work once"
    );
}

/// Test that wrong PIN is rejected
#[tokio::test]
async fn test_wrong_pin_rejected() {
    let base_url = "http://localhost:6968";
    let client = Client::new();

    // Setup: Get token
    let modal_response = client
        .get(format!("{}/mesh/join-modal", base_url))
        .send()
        .await
        .unwrap();

    let modal_html = modal_response.text().await.unwrap();
    let join_url = extract_join_url(&modal_html).unwrap();
    let token = extract_token_from_url(&join_url).unwrap();

    // Access join page to generate PIN
    client.get(&join_url).send().await.unwrap();

    // Try wrong PIN
    let verify_response = client
        .post(format!("{}/mesh/verify-join-pin", base_url))
        .json(&json!({
            "token": token,
            "pin": "000000"  // Wrong PIN
        }))
        .send()
        .await
        .unwrap();

    assert!(
        verify_response.status().is_client_error(),
        "Wrong PIN should be rejected"
    );
}

/// Test HTTP and HTTPS servers both work
#[tokio::test]
async fn test_http_and_https_servers() {
    let http_client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    // HTTP server should serve join page
    let http_response = http_client
        .get("http://localhost:6968/join?token=test")
        .send()
        .await;

    // HTTPS server should also work (but may require cert trust)
    let https_response = http_client
        .get("https://localhost:6969/join?token=test")
        .send()
        .await;

    // Both should return some response (even if error due to invalid token)
    assert!(http_response.is_ok(), "HTTP server should respond");
    assert!(https_response.is_ok(), "HTTPS server should respond");
}

/// Test mobile device redirect after successful pairing
#[tokio::test]
async fn test_mobile_redirect_after_pairing() {
    let base_url = "http://localhost:6968";
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())  // Don't follow redirects
        .build()
        .unwrap();

    // This test verifies that after pairing, the mobile's polling
    // detects the mesh membership and redirects to /dashboard

    // In a real E2E test, we'd:
    // 1. Start a headless browser
    // 2. Load the join page
    // 3. Wait for pairing to complete (desktop enters PIN)
    // 4. Verify JavaScript redirects to /dashboard

    // For now, we test the GraphQL polling logic works
    let nodes_response = client
        .post(format!("{}/graphql", base_url))
        .json(&json!({
            "query": "{ nodes { id } }"
        }))
        .send()
        .await
        .unwrap();

    assert!(nodes_response.status().is_success());
    let result: serde_json::Value = nodes_response.json().await.unwrap();
    assert!(result["data"]["nodes"].is_array());
}

// Helper functions for extracting data from HTML/URLs

fn extract_join_url(html: &str) -> Option<String> {
    // Extract join URL from the modal HTML
    // Look for: <code id="join-url-code">URL</code>
    let start = html.find(r#"id="join-url-code""#)?;
    let code_start = html[start..].find('>')? + start + 1;
    let code_end = html[code_start..].find("</code>")? + code_start;
    Some(html[code_start..code_end].trim().to_string())
}

fn extract_token_from_url(url: &str) -> Option<String> {
    // Extract token parameter from URL
    let token_start = url.find("token=")? + 6;
    let token_end = url[token_start..]
        .find(&['&', '#'][..])
        .map(|pos| token_start + pos)
        .unwrap_or(url.len());
    Some(url[token_start..token_end].to_string())
}

fn extract_pin_from_html(html: &str) -> Option<String> {
    // Extract PIN from join page HTML
    // Look for: <div class="pin-display__code" id="pin-code">123456</div>
    let start = html.find(r#"id="pin-code""#)?;
    let pin_start = html[start..].find('>')? + start + 1;
    let pin_end = html[pin_start..].find("</div>")? + pin_start;
    let pin = html[pin_start..pin_end].trim().to_string();

    // Filter out dashes used as placeholder
    if pin == "------" {
        None
    } else {
        Some(pin)
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_extract_join_url() {
        let html = r#"<code id="join-url-code">http://192.168.1.1:6968/join?token=abc123</code>"#;
        let url = extract_join_url(html).unwrap();
        assert_eq!(url, "http://192.168.1.1:6968/join?token=abc123");
    }

    #[test]
    fn test_extract_token_from_url() {
        let url = "http://192.168.1.1:6968/join?token=abc123xyz";
        let token = extract_token_from_url(url).unwrap();
        assert_eq!(token, "abc123xyz");
    }

    #[test]
    fn test_extract_pin_from_html() {
        let html = r#"<div class="pin-display__code" id="pin-code">582254</div>"#;
        let pin = extract_pin_from_html(html).unwrap();
        assert_eq!(pin, "582254");
    }

    #[test]
    fn test_extract_pin_placeholder() {
        let html = r#"<div class="pin-display__code" id="pin-code">------</div>"#;
        let pin = extract_pin_from_html(html);
        assert!(pin.is_none(), "Placeholder should not be extracted as PIN");
    }
}
