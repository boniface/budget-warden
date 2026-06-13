use budget_warden::{
    BudgetBroker, BudgetPolicy, BudgetRequest, BudgetStrategy, BudgetUnit, BudgetWarden,
    FallbackAction, MemoryStore, PreserveForWindow,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let policy = BudgetPolicy::builder("weatherapi-daily-free-plan")
        .provider("weatherapi")
        .domain("weather")
        .resource("forecast")
        .subject("global")
        .unit(BudgetUnit::Requests)
        .hard_limit(1000)
        .calendar_day("Africa/Lusaka")
        .strategy(BudgetStrategy::PreserveForWindow(PreserveForWindow::new(
            15, 10, None,
        )))
        .exhausted_action(FallbackAction::UseStaleCache)
        .build()?;
    let warden = BudgetWarden::builder()
        .store(MemoryStore::new())
        .policy(policy)
        .build()?;
    let request = BudgetRequest::builder("weatherapi", "weather", "forecast").build()?;

    let _decision = warden.authorize(request).await?;
    Ok(())
}
