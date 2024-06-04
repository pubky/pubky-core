use chrono::{serde::ts_milliseconds_option::deserialize as ts_milliseconds_option, DateTime, Utc};
use crux_core::render::Render;
use serde::{Deserialize, Serialize};

// ANCHOR: model
#[derive(Default, Serialize)]
pub struct Model {
    count: Count,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq, Eq)]
pub struct Count {
    value: isize,
    #[serde(deserialize_with = "ts_milliseconds_option")]
    updated_at: Option<DateTime<Utc>>,
}
// ANCHOR_END: model

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ViewModel {
    pub text: String,
    pub confirmed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Event {}

#[cfg_attr(feature = "typegen", derive(crux_core::macros::Export))]
#[derive(crux_core::macros::Effect)]
pub struct Capabilities {
    pub render: Render<Event>,
}

#[derive(Default)]
pub struct App;

impl crux_core::App for App {
    type Model = Model;
    type Event = Event;
    type ViewModel = ViewModel;
    type Capabilities = Capabilities;

    fn update(&self, _msg: Self::Event, _model: &mut Self::Model, _caps: &Self::Capabilities) {
        // match msg {}
    }

    fn view(&self, model: &Self::Model) -> Self::ViewModel {
        let suffix = match model.count.updated_at {
            None => " (pending)".to_string(),
            Some(d) => format!(" ({d})"),
        };

        Self::ViewModel {
            text: model.count.value.to_string() + &suffix,
            confirmed: model.count.updated_at.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    // use super::{App, Event, Model};
    // use crate::capabilities::sse::SseRequest;
    // use crate::{Count, Effect};
    // use assert_let_bind::assert_let;
    // use chrono::{TimeZone, Utc};
    // use crux_core::{assert_effect, testing::AppTester};
    // use crux_http::protocol::HttpResult;
    // use crux_http::{
    //     protocol::{HttpRequest, HttpResponse},
    //     testing::ResponseBuilder,
    // };
}
