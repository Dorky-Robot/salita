/// E2E tests for dashboard functionality
/// These tests run against a real server instance
use reqwest::Client;
use serde_json::json;

const BASE_URL: &str = "http://localhost:6969";

/// Helper to create an authenticated session
async fn create_test_session(client: &Client) -> Result<String, Box<dyn std::error::Error>> {
    // Use the /test/seed endpoint if SALITA_TEST_SEED is set
    let response = client.get(format!("{}/test/seed", BASE_URL)).send().await?;

    // Extract session cookie
    let cookie_value = response
        .cookies()
        .find(|c| c.name() == "salita_session")
        .map(|c| c.value().to_string());

    cookie_value.ok_or_else(|| "No session cookie returned".into())
}

#[tokio::test]
#[ignore] // Run with: cargo test --test e2e_dashboard -- --ignored
async fn test_dashboard_loads() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().cookie_store(true).build()?;

    // Create session
    let _session = create_test_session(&client).await?;

    // Load dashboard
    let response = client.get(format!("{}/dashboard", BASE_URL)).send().await?;

    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("Mesh Network"));

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_graphql_query_nodes() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().cookie_store(true).build()?;
    let _session = create_test_session(&client).await?;

    // Query nodes via GraphQL
    let response = client
        .post(format!("{}/graphql", BASE_URL))
        .json(&json!({
            "query": "{ nodes { id name status } }"
        }))
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await?;
    assert!(body["data"]["nodes"].is_array());

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_remove_node_mutation() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().cookie_store(true).build()?;
    let _session = create_test_session(&client).await?;

    // First, register a test node
    let register_response = client
        .post(format!("{}/graphql", BASE_URL))
        .json(&json!({
            "query": r#"
                mutation RegisterNode($input: RegisterNodeInput!) {
                    registerNode(input: $input) {
                        success
                        node { id }
                    }
                }
            "#,
            "variables": {
                "input": {
                    "name": "Test Node",
                    "hostname": "192.168.1.100",
                    "port": 6969
                }
            }
        }))
        .send()
        .await?;

    let register_body: serde_json::Value = register_response.json().await?;
    let node_id = register_body["data"]["registerNode"]["node"]["id"]
        .as_str()
        .expect("Node ID should be present");

    // Now remove the node
    let remove_response = client
        .post(format!("{}/graphql", BASE_URL))
        .json(&json!({
            "query": r#"
                mutation RemoveNode($nodeId: String!) {
                    removeNode(nodeId: $nodeId) {
                        success
                        message
                    }
                }
            "#,
            "variables": {
                "nodeId": node_id
            }
        }))
        .send()
        .await?;

    assert_eq!(remove_response.status(), 200);
    let remove_body: serde_json::Value = remove_response.json().await?;
    assert_eq!(remove_body["data"]["removeNode"]["success"], true);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_pin_verification_flow() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().cookie_store(true).build()?;
    let _session = create_test_session(&client).await?;

    // This would require mocking the join token flow
    // Left as a TODO for more complex E2E scenarios

    Ok(())
}
