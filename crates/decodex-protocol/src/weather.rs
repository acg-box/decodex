//! Bounded weather projection from a native tool result.
/// Saved native weather conditions and at most 24 hourly forecast entries.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct WeatherForecast {
	/// Native citation reference that identifies this forecast.
	pub reference: String,
	/// Location reported by the native weather result.
	pub location: String,
	/// Current weather description reported by the provider.
	pub condition: String,
	/// Current temperature in degrees Celsius.
	pub celsius: i32,
	/// Hourly entries as provider time label, weather description, and Celsius temperature.
	pub hours: Vec<(String, String, i32)>,
}
fn temperature(value: &str) -> Option<i32> {
	value.rsplit_once('(')?.1.strip_suffix("°C)")?.parse().ok()
}
impl WeatherForecast {
	/// Parse the supported native weather text and its citation reference.
	pub fn parse(source: &str) -> Option<Self> {
		let (reference, body) =
			source.strip_prefix("\u{e200}cite\u{e202}")?.split_once('\u{e201}')?;
		let mut lines = body.trim().lines();
		let location = lines.next()?.strip_prefix("Weather for ")?.strip_suffix(':')?;
		let current = lines.next()?.strip_prefix("Current Conditions: ")?;
		let (condition, _) = current.rsplit_once(", ")?;
		if lines.next()? != "Hourly Forecast:" {
			return None;
		}
		let hours = lines
			.take(24)
			.map(|line| {
				let (hour, value) = line.split_once(": ")?;
				let (condition, _) = value.rsplit_once(", ")?;
				Some((hour.to_owned(), condition.to_owned(), temperature(value)?))
			})
			.collect::<Option<Vec<_>>>()?;
		if hours.is_empty() {
			return None;
		}
		Some(Self {
			reference: reference.into(),
			location: location.into(),
			condition: condition.into(),
			celsius: temperature(current)?,
			hours,
		})
	}

	/// Render the complete saved forecast as readable Markdown.
	pub fn markdown(&self) -> String {
		let mut result = format!(
			"## {}\n\n{}°C · {}\n\n| Time | Weather | °C |\n| --- | --- | --- |\n",
			self.location, self.celsius, self.condition
		);
		for (hour, condition, temperature) in &self.hours {
			result.push_str(&format!("| {hour} | {condition} | {temperature} |\n"));
		}
		result
	}
}
