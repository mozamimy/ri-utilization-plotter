extern crate chrono;
extern crate env_logger;
#[macro_use]
extern crate lambda_runtime as lambda;
#[macro_use]
extern crate log;
extern crate rusoto_ce;
extern crate rusoto_cloudwatch;
extern crate rusoto_core;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

use rusoto_ce::CostExplorer;
use rusoto_cloudwatch::CloudWatch;

#[derive(Debug, Serialize, Deserialize)]
struct Event {
    region: Option<String>,
    service: Option<String>,
    linked_account: Option<String>,
    granularity: Option<String>,
    namespace: String,
    metric_name: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = env_logger::try_init();
    lambda!(handler);
    Ok(())
}

fn push_filter(event_element: &Option<String>, exps: &mut Vec<rusoto_ce::Expression>, key: String) {
    if event_element.is_some() {
        exps.push(rusoto_ce::Expression {
            dimensions: Some(rusoto_ce::DimensionValues {
                key: Some(key),
                values: Some(vec![event_element.clone().unwrap()]),
            }),
            ..Default::default()
        });
    }
}

fn push_dimension(event_element: &Option<String>, dimensions: &mut Vec<rusoto_cloudwatch::Dimension>, name: String) {
    if event_element.is_some() {
        dimensions.push(rusoto_cloudwatch::Dimension {
            name: name,
            value: event_element.clone().unwrap(),
        });
    }
}

fn handler(event: Event, ctx: lambda::Context) -> Result<String, lambda::error::HandlerError> {
    // CostExplorer API is available in only us-east-1 (https://ce.us-east-1.amazonaws.com/)
    let cost_explorer = rusoto_ce::CostExplorerClient::new(rusoto_core::Region::UsEast1);
    let mut filter = rusoto_ce::Expression {
        and: Some(vec![]),
        ..Default::default()
    };
    push_filter(&event.region, filter.and.as_mut().unwrap(), "REGION".to_string());
    push_filter(&event.service, filter.and.as_mut().unwrap(), "SERVICE".to_string());
    push_filter(&event.linked_account, filter.and.as_mut().unwrap(), "LINKED_ACCOUNT".to_string());
    let today: chrono::Date<chrono::Utc> = chrono::Utc::today();
    let ce_request = rusoto_ce::GetReservationUtilizationRequest {
        filter: Some(filter),
        granularity: event.granularity,
        time_period: rusoto_ce::DateInterval {
            start: (today - chrono::Duration::days(7)).format("%Y-%m-%d").to_string(),
            end: today.format("%Y-%m-%d").to_string(),
        },
        ..Default::default()
    };

    info!("Make a request for Cost Explorer");
    info!("{:?}", ce_request);

    let percentage: String;
    match cost_explorer.get_reservation_utilization(ce_request).sync() {
        Ok(r) => {
            percentage = r
                .utilizations_by_time
                .last()
                .unwrap()
                .total
                .as_ref()
                .unwrap()
                .utilization_percentage
                .as_ref()
                .unwrap()
                .to_string();
        }
        Err(e) => return Err(ctx.new_error(&format!("{:?}", e))),
    }

    let cloudwatch = rusoto_cloudwatch::CloudWatchClient::new(rusoto_core::Region::default());
    let mut dimensions = vec![];
    push_dimension(&event.region, &mut dimensions, "Region".to_string());
    push_dimension(&event.service, &mut dimensions, "Service".to_string());
    push_dimension(&event.linked_account, &mut dimensions, "LinkedAccount".to_string());
    let metric_data = vec![rusoto_cloudwatch::MetricDatum {
        metric_name: event.metric_name,
        value: Some(percentage.parse().unwrap()),
        dimensions: Some(dimensions),
        ..Default::default()
    }];
    let cw_metric_input = rusoto_cloudwatch::PutMetricDataInput {
        namespace: event.namespace,
        metric_data: metric_data,
    };

    info!("Make a request for CloudWatch Metrics");
    info!("{:?}", cw_metric_input);

    match cloudwatch.put_metric_data(cw_metric_input).sync() {
        Ok(r) => return Ok(format!("{:?}", r)),
        Err(e) => return Err(ctx.new_error(&format!("{:?}", e))),
    }
}