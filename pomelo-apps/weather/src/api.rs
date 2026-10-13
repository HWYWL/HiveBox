//! The requests this page makes, and the readings it makes of the answers.
//!
//! # Two services, each named once
//!
//! Open-Meteo's forecast endpoint and ip-api's locator, because neither needs a key and no account: a
//! box that has just been plugged in has no credential to be given and nothing to sign up for, and a
//! page that needed one would be a page that is blank until somebody fills in a form on another
//! machine. Each endpoint is a `const` and the URLs are built from them, so the services this page
//! talks to are one line each to change.
//!
//! # The URL *is* the request
//!
//! [`HttpBackend`][pomelo_hal::HttpBackend] takes a URL and nothing else — see the trait's own note —
//! so which variables are asked for, for which coordinates and for how many days are all in the
//! string [`forecast_url`] builds. That is also what makes the request testable without a network:
//! the simulator keeps the URL it was handed ([`SimHttp::last_url`][pomelo_hal::sim::SimHttp::last_url]),
//! so a test can check the question rather than only the answer.
//!
//! # Reading the answer is a second judgement
//!
//! The network's faults are the HAL's ([`HttpFault`]); this module's are the *document's*. They are
//! kept apart because they need different things from a person: one is a Wi-Fi switch and the other
//! is "try again in a minute", and a page that drew them the same way would send somebody to look at
//! the wrong thing.

use crate::json::{self, Json};

/// The forecast endpoint.
pub const ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";

/// How many days the page asks for, today included.
///
/// Five, which is what fits the panel without scrolling: the list is read at a glance and a forecast
/// whose last row is below the fold is a forecast of four days. The service's own limit is sixteen.
pub const DAYS: usize = 5;

/// Where this box is, as somebody else's server sees it.
///
/// A second service, and it earns its place: nothing on the board knows its own city — there is no GPS
/// and no location service behind the HAL — and the fixed point this page used to fall back on was a
/// city somebody typed into this file, drawn as if it were the reader's own. The address the router
/// handed out is something the network *does* know, and ip-api turns it into a place.
///
/// Plain HTTP, which is this service's own shape rather than a preference: the free tier of ip-api is
/// HTTP-only and its TLS endpoint wants a key. Nothing secret travels either way — the question is the
/// connection's own address — so one round trip without a handshake is the right request for the one
/// the page makes on its way to the forecast.
///
/// The `fields` are the ones this page reads, and naming them is not tidiness: the service's whole
/// answer carries an ISP, an AS number and a time zone, and a body several times the size is a body
/// this board buffers in order to read four words of it.
pub const LOCATOR: &str =
    "http://ip-api.com/json/?fields=status,message,city,regionName,country,lat,lon";

/// Where the forecast is for.
///
/// A place on the globe and the name to draw above it. The two travel together because they are one
/// fact — a coordinate with somebody else's name on it is a page that says it is raining in the
/// wrong city.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// What the header calls it. Data rather than interface text: a city's name is not translated.
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Location {
    pub fn new(name: impl Into<String>, latitude: f64, longitude: f64) -> Self {
        Self {
            name: name.into(),
            latitude,
            longitude,
        }
    }
}

impl Default for Location {
    /// Nowhere, until [`parse_location`] has named somewhere.
    ///
    /// There is no GPS and no location service behind [`pomelo_hal::Board`], and a page that derived a
    /// city from the time zone would be inventing one — an offset of `+8` is a great many places. The
    /// board does not have to invent one now that the locator can answer, so the page opens with no
    /// place at all rather than with a city out of this file: the empty name is how the header knows
    /// to draw nothing instead of drawing a forecast for somewhere the reader is not (see
    /// [`crate::ui`]).
    ///
    /// The zeroes are that same statement in numbers. They are not a place, and nothing ever asks the
    /// forecast service about them: a forecast is requested only for a location a locator has named,
    /// so the only thing this default is read for is drawing a page that has not been answered yet.
    fn default() -> Self {
        Self::new("", 0.0, 0.0)
    }
}

/// The request, as the URL the client is handed.
pub fn forecast_url(location: &Location) -> String {
    // Coordinates to four decimals, which is about eleven metres: more than a weather model has, and
    // few enough digits that the URL stays readable in a log.
    format!(
        "{ENDPOINT}?latitude={:.4}&longitude={:.4}\
         &current=temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m\
         &daily=weather_code,temperature_2m_max,temperature_2m_min\
         &timezone=auto&forecast_days={DAYS}",
        location.latitude, location.longitude
    )
}

/// Reads `body` as the locator's answer: which place this box's address is in.
///
/// Fails the same two ways a forecast does, and for the same reasons — the bytes may not be JSON, or
/// JSON of another shape — with one addition. This service answers `200` whether or not it could place
/// the address and says which in the document's own `status`, and that "no" is [`Fault::Shape`] rather
/// than a third reason: the document is the one this service documents, saying it cannot help. What
/// the page does about it is the same either way, which is to say so.
///
/// The name is the city, and the region where there is no city: a town small enough to be answered
/// with an empty `city` is still in a province, and a header naming the province is worth more than a
/// header naming nothing.
pub fn parse_location(body: &str) -> Result<Location, Fault> {
    let document = json::parse(body).map_err(Fault::Json)?;

    if document.get("status").and_then(Json::string) != Some("success") {
        return Err(Fault::Shape);
    }

    let latitude = number(&document, "lat")?;
    let longitude = number(&document, "lon")?;
    let name = ["city", "regionName", "country"]
        .iter()
        .filter_map(|key| document.get(key).and_then(Json::string))
        .find(|text| !text.is_empty())
        .ok_or(Fault::Shape)?;

    Ok(Location::new(name, latitude, longitude))
}

/// Why an answer was not what the page asked for.
///
/// Two, and they are different jobs: a document that is not JSON at all usually means something else
/// answered — a captive portal, a proxy's error page — while a document of the wrong *shape* is the
/// service having changed what it sends. Neither changes what the page does, which is why they are
/// two reasons and one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The bytes were not a JSON document.
    Json(json::JsonFault),
    /// A document, and not the shape this service documents: a field missing, or one of a kind this
    /// page cannot read.
    Shape,
}

/// The weather the page draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    pub current: Current,
    /// The days ahead, today first, in the order the service sent them.
    pub days: Vec<Day>,
}

impl Forecast {
    /// Today's range, which is the first day the service sent.
    pub fn today(&self) -> Option<&Day> {
        self.days.first()
    }
}

/// What it is like out, right now.
///
/// No `time` field, though the answer has one: a reading this page draws *now* with the instant the
/// service took it is two clocks to reconcile, and the page has no use for the second one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Current {
    pub temperature_c: f32,
    /// The relative humidity, in `0.0..=100.0`, as a float because that is what the document holds:
    /// rounding it to an integer here would be this module deciding how a page draws a percentage.
    pub humidity_percent: f32,
    pub wind_kmh: f32,
    pub condition: Condition,
}

/// One day of the forecast.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    /// The day, as the service spells it (`2026-10-12`). Data, and the page's own business to turn
    /// into a label — see `ui::day_label`.
    pub date: String,
    /// The day's weather, as one code standing for its worst few hours: what the service reports for
    /// a day is one code, and it is the code that matters rather than an average of them.
    pub condition: Condition,
    pub high_c: f32,
    pub low_c: f32,
}

/// What the sky is doing, as the handful of states this page draws.
///
/// The service sends a WMO weather code — a closed table of about thirty values — and this is that
/// table folded down to the states a person names: nobody looking out of a window distinguishes
/// "slight" from "moderate" drizzle, and a row that did would be a row about a number rather than
/// about the weather.
///
/// [`Condition::Unknown`] is a real variant and not a panic: the table is extended from time to time,
/// and a page that drew an extended code as `Clear` would be lying about the sky. `—` is the honest
/// drawing of "this board does not know what that code means".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    FreezingRain,
    Rain,
    Snow,
    Showers,
    Thunderstorm,
    Unknown,
}

impl Condition {
    /// The condition a WMO weather code stands for.
    ///
    /// The ranges are the ones the table is written in — `51..=55` drizzle, `61..=65` rain — and the
    /// gaps between them are codes the table does not use.
    pub fn of_code(code: u32) -> Self {
        match code {
            0 => Self::Clear,
            1 | 2 => Self::PartlyCloudy,
            3 => Self::Overcast,
            45 | 48 => Self::Fog,
            51 | 53 | 55 => Self::Drizzle,
            // Freezing drizzle and freezing rain are one condition here: what makes them worth their
            // own icon is the ice, and which of the two made it is not what a person acts on.
            56 | 57 | 66 | 67 => Self::FreezingRain,
            61 | 63 | 65 => Self::Rain,
            71 | 73 | 75 | 77 | 85 | 86 => Self::Snow,
            80 | 81 | 82 => Self::Showers,
            95 | 96 | 99 => Self::Thunderstorm,
            _ => Self::Unknown,
        }
    }
}

/// Reads `body` as a forecast.
pub fn parse(body: &str) -> Result<Forecast, Fault> {
    let document = json::parse(body).map_err(Fault::Json)?;

    let current = document.get("current").ok_or(Fault::Shape)?;
    let daily = document.get("daily").ok_or(Fault::Shape)?;

    Ok(Forecast {
        current: Current {
            temperature_c: number(current, "temperature_2m")? as f32,
            humidity_percent: number(current, "relative_humidity_2m")? as f32,
            wind_kmh: number(current, "wind_speed_10m")? as f32,
            condition: Condition::of_code(number(current, "weather_code")? as u32),
        },
        days: days(daily)?,
    })
}

/// The days of the forecast, out of the four arrays the service sends them in.
///
/// Open-Meteo is a column store: the dates and each of the measurements are separate arrays that line
/// up by index, which is why this reads four arrays and walks them together rather than reading a
/// list of day objects. The length is the *shortest* of the four, and that is the honest reading of a
/// truncated answer: a day that is missing one of its numbers is not a day this page can draw, and
/// inventing the missing one would put a temperature on the page that nothing reported.
fn days(daily: &Json) -> Result<Vec<Day>, Fault> {
    let column = |key: &str| -> Result<&[Json], Fault> {
        daily.get(key).and_then(Json::array).ok_or(Fault::Shape)
    };

    let times = column("time")?;
    let codes = column("weather_code")?;
    let highs = column("temperature_2m_max")?;
    let lows = column("temperature_2m_min")?;

    let count = times
        .len()
        .min(codes.len())
        .min(highs.len())
        .min(lows.len());

    (0..count)
        .map(|at| {
            Ok(Day {
                date: times[at].string().ok_or(Fault::Shape)?.to_string(),
                condition: Condition::of_code(codes[at].number().ok_or(Fault::Shape)? as u32),
                high_c: highs[at].number().ok_or(Fault::Shape)? as f32,
                low_c: lows[at].number().ok_or(Fault::Shape)? as f32,
            })
        })
        .collect()
}

/// The number a field of an object is, or [`Fault::Shape`] if it is not one.
fn number(object: &Json, key: &str) -> Result<f64, Fault> {
    object
        .get(key)
        .and_then(Json::number)
        .ok_or(Fault::Shape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::SAMPLE;

    // The answer these tests read is [`crate::tests::SAMPLE`], beside the page tests that use the same
    // document: one fixture, so that a change to what the service sends breaks both in one place.

    /// The question, which is the whole of what a request is here.
    #[test]
    fn the_url_carries_the_coordinates_the_fields_and_the_days() {
        let url = forecast_url(&Location::new("Beijing", 39.9042, 116.4074));

        assert!(url.starts_with(ENDPOINT), "the endpoint, and no other");
        assert!(url.contains("latitude=39.9042"), "{url}");
        assert!(url.contains("longitude=116.4074"), "{url}");
        assert!(url.contains("temperature_2m"), "the reading this page is about");
        assert!(url.contains("relative_humidity_2m"), "{url}");
        assert!(url.contains("weather_code"), "{url}");
        assert!(url.contains("wind_speed_10m"), "{url}");
        assert!(url.contains("timezone=auto"), "so the days are local ones");
        assert!(url.contains(&format!("forecast_days={DAYS}")), "{url}");

        // Nowhere else's coordinates, and nothing the URL has to be put together to find.
        assert!(
            !url.contains(' '),
            "a URL with a space in it is a URL that gets truncated"
        );
    }

    /// A coordinate is in the URL the way it was given, and not rounded to a whole degree.
    #[test]
    fn a_location_is_asked_for_where_it_is() {
        let url = forecast_url(&Location::new("Berlin", 52.52, 13.405));
        assert!(url.contains("latitude=52.5200"), "{url}");
        assert!(url.contains("longitude=13.4050"), "{url}");
    }

    /// The reading, out of the document the service sends.
    #[test]
    fn the_forecast_is_read_out_of_the_answer() {
        let forecast = parse(SAMPLE).expect("the sample is a forecast");

        assert_eq!(forecast.current.temperature_c, 18.4);
        assert_eq!(forecast.current.humidity_percent, 46.0);
        assert_eq!(forecast.current.wind_kmh, 11.5);
        assert_eq!(forecast.current.condition, Condition::PartlyCloudy);

        assert_eq!(forecast.days.len(), 5);
        assert_eq!(forecast.today().unwrap().date, "2026-10-12");
        assert_eq!(forecast.days[2].condition, Condition::Rain, "code 61");
        assert_eq!(forecast.days[2].high_c, 16.2);
        assert_eq!(forecast.days[2].low_c, 8.8);
    }

    /// The four arrays are columns of one table, and a row is read by index across them.
    #[test]
    fn the_columns_are_walked_together() {
        let body = r#"{"current":{"temperature_2m":1,"relative_humidity_2m":2,"weather_code":0,"wind_speed_10m":3},
                       "daily":{"time":["a","b","c"],"weather_code":[0,45,95],
                                "temperature_2m_max":[1,2,3],"temperature_2m_min":[-1,-2,-3]}}"#;

        let forecast = parse(body).unwrap();

        assert_eq!(
            forecast
                .days
                .iter()
                .map(|day| (day.date.as_str(), day.condition, day.high_c, day.low_c))
                .collect::<Vec<_>>(),
            vec![
                ("a", Condition::Clear, 1.0, -1.0),
                ("b", Condition::Fog, 2.0, -2.0),
                ("c", Condition::Thunderstorm, 3.0, -3.0),
            ]
        );
    }

    /// A truncated column shortens the forecast rather than inventing a day.
    #[test]
    fn a_short_column_is_a_shorter_forecast() {
        let body = r#"{"current":{"temperature_2m":1,"relative_humidity_2m":2,"weather_code":0,"wind_speed_10m":3},
                       "daily":{"time":["a","b","c"],"weather_code":[0,45,95],
                                "temperature_2m_max":[1,2],"temperature_2m_min":[-1,-2,-3]}}"#;

        let forecast = parse(body).unwrap();
        assert_eq!(forecast.days.len(), 2, "the shortest column is the answer");
        assert_eq!(forecast.days[1].date, "b");
    }

    /// A document that is not this service's answer is a fault and not an empty forecast.
    ///
    /// The one thing that must not happen: a page drawing a sunny day because a field was missing.
    #[test]
    fn what_is_not_a_forecast_is_refused() {
        // Not JSON at all: what a captive portal sends.
        assert_eq!(parse("<html>Sign in</html>"), Err(Fault::Json(json::JsonFault::Unexpected)));
        assert_eq!(parse(""), Err(Fault::Json(json::JsonFault::Truncated)));

        // JSON, and not this shape.
        assert_eq!(parse("{}"), Err(Fault::Shape));
        assert_eq!(parse(r#"{"current":{}}"#), Err(Fault::Shape));

        let missing_daily = r#"{"current":{"temperature_2m":1,"relative_humidity_2m":2,
                                           "weather_code":0,"wind_speed_10m":3}}"#;
        assert_eq!(parse(missing_daily), Err(Fault::Shape));

        // A field of the wrong kind: a temperature written as a string is not a temperature.
        let text_temperature = r#"{"current":{"temperature_2m":"18.4","relative_humidity_2m":2,
                                              "weather_code":0,"wind_speed_10m":3},
                                  "daily":{"time":[],"weather_code":[],
                                           "temperature_2m_max":[],"temperature_2m_min":[]}}"#;
        assert_eq!(parse(text_temperature), Err(Fault::Shape));
    }

    /// The whole WMO table this page knows, and the codes it does not.
    #[test]
    fn the_weather_codes_fold_into_the_states_a_person_names() {
        let mapped = [
            (0, Condition::Clear),
            (1, Condition::PartlyCloudy),
            (2, Condition::PartlyCloudy),
            (3, Condition::Overcast),
            (45, Condition::Fog),
            (48, Condition::Fog),
            (51, Condition::Drizzle),
            (55, Condition::Drizzle),
            (56, Condition::FreezingRain),
            (57, Condition::FreezingRain),
            (61, Condition::Rain),
            (65, Condition::Rain),
            (66, Condition::FreezingRain),
            (67, Condition::FreezingRain),
            (71, Condition::Snow),
            (75, Condition::Snow),
            (77, Condition::Snow),
            (80, Condition::Showers),
            (82, Condition::Showers),
            (85, Condition::Snow),
            (86, Condition::Snow),
            (95, Condition::Thunderstorm),
            (96, Condition::Thunderstorm),
            (99, Condition::Thunderstorm),
        ];

        for (code, condition) in mapped {
            assert_eq!(Condition::of_code(code), condition, "code {code}");
        }

        // The gaps in the table, and a code from a future revision of it: the page says it does not
        // know rather than picking the nearest thing.
        for code in [4, 30, 44, 50, 60, 70, 90, 100, 1000] {
            assert_eq!(Condition::of_code(code), Condition::Unknown, "code {code}");
        }
    }

    /// The default location is nowhere, and the empty name is how the header knows not to draw one.
    ///
    /// This page no longer has a city of its own to fall back on: a hard-coded one drawn before the
    /// locator answered would be a forecast for somewhere the reader is not, which is the fault this
    /// default exists to avoid.
    #[test]
    fn the_default_location_is_nowhere() {
        let location = Location::default();

        assert_eq!(location.name, "");
        assert_eq!(location.latitude, 0.0);
        assert_eq!(location.longitude, 0.0);
    }

    /// The locator's answer, read for the place it names and the point it is at.
    #[test]
    fn the_locators_answer_is_read_as_a_place() {
        let location = parse_location(crate::tests::LOCATED).expect("the sample places the address");

        assert_eq!(location.name, "Lisbon", "the city, not the region or the country");
        assert_eq!(location.latitude, 38.7167);
        assert_eq!(location.longitude, -9.1333);
    }

    /// A town small enough to have no city is named by its region, and only then by its country.
    #[test]
    fn a_place_with_no_city_is_named_by_the_next_thing_it_has() {
        let region = r#"{"status":"success","country":"Portugal","regionName":"Lisboa",
                          "city":"","lat":38.7,"lon":-9.1}"#;
        assert_eq!(parse_location(region).unwrap().name, "Lisboa");

        let country = r#"{"status":"success","country":"Portugal","regionName":"","city":"",
                          "lat":38.7,"lon":-9.1}"#;
        assert_eq!(parse_location(country).unwrap().name, "Portugal");
    }

    /// A locator that cannot place the address is refused, and so is a document of another shape.
    ///
    /// The service answers `200` whether or not it could help, so the `status` field is the answer:
    /// reading `lat` out of a document that says `fail` would be drawing a forecast at whatever
    /// coordinates a failure happened to leave in it.
    #[test]
    fn what_is_not_a_location_is_refused() {
        assert_eq!(
            parse_location(r#"{"status":"fail","message":"reserved range","query":"192.168.1.1"}"#),
            Err(Fault::Shape)
        );
        assert_eq!(parse_location("{}"), Err(Fault::Shape));
        assert_eq!(parse_location("<html>Sign in</html>"), Err(Fault::Json(json::JsonFault::Unexpected)));
        assert_eq!(parse_location(""), Err(Fault::Json(json::JsonFault::Truncated)));

        // A success with no name to draw, or a coordinate written as a string, is not a place either.
        assert_eq!(
            parse_location(r#"{"status":"success","country":"","regionName":"","city":"",
                              "lat":38.7,"lon":-9.1}"#),
            Err(Fault::Shape)
        );
        assert_eq!(
            parse_location(r#"{"status":"success","city":"Lisbon","lat":"38.7167","lon":-9.1333}"#),
            Err(Fault::Shape)
        );
    }
}
