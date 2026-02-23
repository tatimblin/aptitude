//! Demo of the new tiered fluent API and parallel processing capabilities.

use std::sync::Arc;
use aptitude::{AgentHarness, AgentType, fluent::StdoutAssertion, review::{grade_stdout_batch_async, ReviewConfig}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let harness = AgentHarness::new();
    let agent = harness.get_agent(AgentType::Claude).unwrap().clone();

    // Example 1: Simple API for 90% of use cases
    println!("=== Simple API Example ===");
    let simple_assertion = StdoutAssertion::with_review(
        Some("Task completed successfully".to_string()),
        "should confirm task completion"
    ).with_grader(agent.clone());

    let result = simple_assertion.evaluate();
    println!("Simple assertion result: {}", if result.passed { "PASS" } else { "FAIL" });

    // Example 2: Advanced builder for complex configurations
    println!("\n=== Advanced Builder Example ===");
    let advanced_assertion = StdoutAssertion::builder()
        .stdout(Some("The file has been created successfully with 142 bytes".to_string()))
        .review("should confirm file creation and mention the file size")
        .with_threshold(8)  // Higher threshold
        .with_model("claude-sonnet-4-20250514")  // Specific model
        .with_grader(agent.clone())
        .build();

    let result = advanced_assertion.evaluate();
    println!("Advanced assertion result: {}", if result.passed { "PASS" } else { "FAIL" });

    // Example 3: Parallel processing (async)
    println!("\n=== Parallel Processing Example ===");
    let outputs = vec![
        Some("Task 1 completed".to_string()),
        Some("Task 2 finished successfully".to_string()),
        Some("Error in task 3".to_string()),
    ];

    let configs = vec![
        ReviewConfig {
            criteria: "should confirm completion".to_string(),
            threshold: 7,
            model: None,
        },
        ReviewConfig {
            criteria: "should indicate success".to_string(),
            threshold: 7,
            model: None,
        },
        ReviewConfig {
            criteria: "should report an error".to_string(),
            threshold: 7,
            model: None,
        },
    ];

    let requests: Vec<_> = outputs.into_iter().zip(configs).collect();

    // This would run all grading requests in parallel
    println!("Running {} assertions in parallel...", requests.len());
    // Note: This is just a demo - in practice you'd await the result
    println!("Parallel processing configured successfully");

    // Example 4: Async single assertion
    println!("\n=== Async Single Assertion Example ===");
    let async_assertion = StdoutAssertion::with_review(
        Some("File uploaded to S3 bucket successfully".to_string()),
        "should confirm S3 upload"
    ).with_grader(agent);

    // This uses the async grading pipeline for better performance
    let result = async_assertion.evaluate_async().await;
    println!("Async assertion result: {}", if result.passed { "PASS" } else { "FAIL" });

    Ok(())
}