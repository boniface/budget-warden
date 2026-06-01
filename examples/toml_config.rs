use budget_warden::{BudgetBroker, BudgetRequest, BudgetWarden};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let warden = BudgetWarden::from_toml_file("examples/config/serpapi_free_plan.toml")?;
    let request = BudgetRequest::builder("serpapi", "search", "google-search")
        .subject("global")
        .build()?;

    let _decision = warden.authorize(request).await?;
    Ok(())
}
