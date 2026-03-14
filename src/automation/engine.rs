//! Browser Form Automation Engine — deterministic form-filling workflows.
//!
//! Provides structured form interaction: field detection, input strategies,
//! multi-step wizard navigation, autocomplete handling, and calendar widgets.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Field Types ──────────────────────────────────────────────────────────────

/// Represents the semantic type of an HTML form field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    Text,
    Email,
    Password,
    Number,
    Phone,
    Date,
    Select,
    Checkbox,
    Radio,
    Textarea,
    File,
    Hidden,
    Search,
    Url,
    Color,
    Range,
    Unknown,
}

impl FieldType {
    /// Map an HTML `type` attribute string to the corresponding [`FieldType`].
    pub fn from_html_type(html_type: &str) -> Self {
        match html_type.to_lowercase().as_str() {
            "text" => Self::Text,
            "email" => Self::Email,
            "password" => Self::Password,
            "number" | "tel" => Self::Number,
            "phone" => Self::Phone,
            "date" | "datetime-local" | "month" | "week" | "time" => Self::Date,
            "select" | "select-one" | "select-multiple" => Self::Select,
            "checkbox" => Self::Checkbox,
            "radio" => Self::Radio,
            "textarea" => Self::Textarea,
            "file" => Self::File,
            "hidden" => Self::Hidden,
            "search" => Self::Search,
            "url" => Self::Url,
            "color" => Self::Color,
            "range" => Self::Range,
            _ => Self::Unknown,
        }
    }
}

// ── Form Field ───────────────────────────────────────────────────────────────

/// A single option within a `<select>` element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub text: String,
    pub selected: bool,
}

/// Describes one field detected inside a form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub selector: String,
    pub field_type: FieldType,
    pub name: String,
    pub label: String,
    pub placeholder: String,
    pub required: bool,
    pub value: String,
    pub options: Vec<SelectOption>,
    pub validation_pattern: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub autocomplete: Option<String>,
}

impl FormField {
    /// Create a minimal field with only a CSS selector and type.
    pub fn new(selector: &str, field_type: FieldType) -> Self {
        Self {
            selector: selector.to_string(),
            field_type,
            name: String::new(),
            label: String::new(),
            placeholder: String::new(),
            required: false,
            value: String::new(),
            options: Vec::new(),
            validation_pattern: None,
            min: None,
            max: None,
            autocomplete: None,
        }
    }

    /// Returns `true` when no value has been entered.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Returns `true` when the field can accept user-supplied data.
    /// Hidden and File fields are excluded.
    pub fn is_fillable(&self) -> bool {
        !matches!(self.field_type, FieldType::Hidden | FieldType::File)
    }
}

// ── Input Strategy ───────────────────────────────────────────────────────────

/// How the engine should interact with a field to set its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputStrategy {
    DirectType,
    ClearAndType,
    SelectOption,
    ClickCheckbox,
    ClickRadio,
    SetDate,
    UploadFile,
    JavaScriptSet,
    AutocompleteSelect,
}

impl InputStrategy {
    /// Choose the best strategy for a given field based on its type and state.
    pub fn for_field(field: &FormField) -> Self {
        match field.field_type {
            FieldType::Select => Self::SelectOption,
            FieldType::Checkbox => Self::ClickCheckbox,
            FieldType::Radio => Self::ClickRadio,
            FieldType::Date => Self::SetDate,
            FieldType::File => Self::UploadFile,
            _ if field.autocomplete.is_some() => Self::AutocompleteSelect,
            _ if !field.value.is_empty() => Self::ClearAndType,
            _ => Self::DirectType,
        }
    }
}

// ── Form Analysis ────────────────────────────────────────────────────────────

/// The result of analysing an HTML form — lists all detected fields and
/// structural properties (multi-step, captcha, file upload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormAnalysis {
    pub form_selector: String,
    pub fields: Vec<FormField>,
    pub submit_selector: Option<String>,
    pub is_multi_step: bool,
    pub current_step: usize,
    pub total_steps: usize,
    pub has_captcha: bool,
    pub has_file_upload: bool,
}

impl FormAnalysis {
    pub fn new(form_selector: &str) -> Self {
        Self {
            form_selector: form_selector.to_string(),
            fields: Vec::new(),
            submit_selector: None,
            is_multi_step: false,
            current_step: 0,
            total_steps: 1,
            has_captcha: false,
            has_file_upload: false,
        }
    }

    /// All fields that can accept user input.
    pub fn fillable_fields(&self) -> Vec<&FormField> {
        self.fields.iter().filter(|f| f.is_fillable()).collect()
    }

    /// All fields marked as required.
    pub fn required_fields(&self) -> Vec<&FormField> {
        self.fields.iter().filter(|f| f.required).collect()
    }

    /// Required fields that have not yet been filled.
    pub fn empty_required_fields(&self) -> Vec<&FormField> {
        self.fields
            .iter()
            .filter(|f| f.required && f.is_empty())
            .collect()
    }

    /// Total number of fields (including hidden / non-fillable).
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Fraction of fillable fields that already have a value (0.0 – 1.0).
    pub fn completion_ratio(&self) -> f64 {
        let fillable: Vec<_> = self.fillable_fields();
        if fillable.is_empty() {
            return 1.0;
        }
        let filled = fillable.iter().filter(|f| !f.is_empty()).count();
        filled as f64 / fillable.len() as f64
    }
}

// ── Fill Plan ────────────────────────────────────────────────────────────────

/// One atomic interaction the engine should perform on a field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillAction {
    pub field_selector: String,
    pub strategy: InputStrategy,
    pub value: String,
    pub delay_ms: u64,
    pub verify_after: bool,
}

/// An ordered sequence of [`FillAction`]s, optionally followed by a submit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillPlan {
    pub actions: Vec<FillAction>,
    pub submit_after: bool,
    pub submit_selector: Option<String>,
}

impl FillPlan {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            submit_after: false,
            submit_selector: None,
        }
    }

    pub fn add_action(&mut self, action: FillAction) {
        self.actions.push(action);
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }
}

impl Default for FillPlan {
    fn default() -> Self {
        Self::new()
    }
}

// ── Autocomplete Strategy ────────────────────────────────────────────────────

/// How the engine should interact with an autocomplete dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutocompleteStrategy {
    TypeAndWait,
    TypeAndArrowDown,
    TypeAndClick,
    TypeAndTab,
}

// ── Calendar Widget ──────────────────────────────────────────────────────────

/// Tracks the state of a calendar / date-picker widget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarState {
    pub is_open: bool,
    pub current_month: u32,
    pub current_year: i32,
    pub selected_date: Option<String>,
    pub min_date: Option<String>,
    pub max_date: Option<String>,
}

impl CalendarState {
    pub fn new() -> Self {
        Self {
            is_open: false,
            current_month: 1,
            current_year: 2024,
            selected_date: None,
            min_date: None,
            max_date: None,
        }
    }

    /// Number of months to navigate (positive = forward, negative = backward)
    /// to reach the target month/year from the current position.
    pub fn months_until(&self, target_month: u32, target_year: i32) -> i32 {
        (target_year - self.current_year) * 12
            + (target_month as i32 - self.current_month as i32)
    }
}

impl Default for CalendarState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Wizard (Multi-Step Form) ─────────────────────────────────────────────────

/// Lifecycle status of one wizard step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WizardStepStatus {
    NotReached,
    Current,
    Completed,
    Skipped,
    Failed,
}

/// A single step inside a multi-page wizard form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardStep {
    pub index: usize,
    pub name: String,
    pub status: WizardStepStatus,
    pub fields: Vec<FormField>,
    pub next_selector: Option<String>,
    pub back_selector: Option<String>,
}

/// Tracks overall wizard progress (step list + cursor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardState {
    pub steps: Vec<WizardStep>,
    pub current_step: usize,
}

impl WizardState {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            current_step: 0,
        }
    }

    /// Append a new step and return its index. The first step added is
    /// automatically marked [`WizardStepStatus::Current`].
    pub fn add_step(&mut self, name: &str) -> usize {
        let idx = self.steps.len();
        self.steps.push(WizardStep {
            index: idx,
            name: name.to_string(),
            status: if idx == 0 {
                WizardStepStatus::Current
            } else {
                WizardStepStatus::NotReached
            },
            fields: Vec::new(),
            next_selector: None,
            back_selector: None,
        });
        idx
    }

    /// Move to the next step. Returns `false` if already at the last step.
    pub fn advance(&mut self) -> bool {
        if self.current_step + 1 < self.steps.len() {
            self.steps[self.current_step].status = WizardStepStatus::Completed;
            self.current_step += 1;
            self.steps[self.current_step].status = WizardStepStatus::Current;
            true
        } else {
            false
        }
    }

    /// Move back one step. Returns `false` if already at the first step.
    pub fn go_back(&mut self) -> bool {
        if self.current_step > 0 {
            self.steps[self.current_step].status = WizardStepStatus::NotReached;
            self.current_step -= 1;
            self.steps[self.current_step].status = WizardStepStatus::Current;
            true
        } else {
            false
        }
    }

    /// All steps are either completed or skipped.
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| {
            s.status == WizardStepStatus::Completed
                || s.status == WizardStepStatus::Skipped
        })
    }

    /// Fraction of steps that are completed or skipped (0.0 – 1.0).
    pub fn progress(&self) -> f64 {
        if self.steps.is_empty() {
            return 0.0;
        }
        let done = self
            .steps
            .iter()
            .filter(|s| {
                s.status == WizardStepStatus::Completed
                    || s.status == WizardStepStatus::Skipped
            })
            .count();
        done as f64 / self.steps.len() as f64
    }

    /// Reference to the step at the current cursor position.
    pub fn current(&self) -> Option<&WizardStep> {
        self.steps.get(self.current_step)
    }

    /// Total number of steps.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

impl Default for WizardState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Fill Result ──────────────────────────────────────────────────────────────

/// Outcome of a fill operation — how many fields succeeded, how many failed,
/// and any error messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillResult {
    pub success: bool,
    pub fields_filled: usize,
    pub fields_failed: usize,
    pub errors: Vec<String>,
    pub duration_ms: f64,
}

impl FillResult {
    /// Create a fully-successful result.
    pub fn success(fields_filled: usize, duration_ms: f64) -> Self {
        Self {
            success: true,
            fields_filled,
            fields_failed: 0,
            errors: Vec::new(),
            duration_ms,
        }
    }

    /// Create a result with potential partial failure.
    pub fn partial(
        fields_filled: usize,
        fields_failed: usize,
        errors: Vec<String>,
        duration_ms: f64,
    ) -> Self {
        Self {
            success: fields_failed == 0,
            fields_filled,
            fields_failed,
            errors,
            duration_ms,
        }
    }
}

// ── Form Automation Engine ───────────────────────────────────────────────────

/// Tuning knobs for the automation engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    pub typing_delay_ms: u64,
    pub action_delay_ms: u64,
    pub wait_timeout_ms: u64,
    pub verify_fills: bool,
    pub handle_autocomplete: bool,
    pub max_retries: usize,
    pub screenshot_on_error: bool,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            typing_delay_ms: 50,
            action_delay_ms: 200,
            wait_timeout_ms: 5000,
            verify_fills: true,
            handle_autocomplete: true,
            max_retries: 3,
            screenshot_on_error: true,
        }
    }
}

/// Top-level engine that analyses forms, builds fill plans, and tracks
/// historical success rates.
pub struct FormAutomationEngine {
    config: AutomationConfig,
    field_matchers: HashMap<String, String>,
    autocomplete_strategies: HashMap<String, AutocompleteStrategy>,
    fill_history: Vec<FillResult>,
}

impl FormAutomationEngine {
    pub fn new(config: AutomationConfig) -> Self {
        Self {
            config,
            field_matchers: Self::default_field_matchers(),
            autocomplete_strategies: HashMap::new(),
            fill_history: Vec::new(),
        }
    }

    /// Built-in regex pattern -> human-readable label mappings for common
    /// form field names.
    fn default_field_matchers() -> HashMap<String, String> {
        let mut m = HashMap::new();
        for (pattern, label) in [
            ("first.?name", "First Name"),
            ("last.?name", "Last Name"),
            ("full.?name", "Full Name"),
            ("email", "Email"),
            ("phone|tel", "Phone"),
            ("address|street", "Address"),
            ("city", "City"),
            ("state|province", "State"),
            ("zip|postal", "ZIP Code"),
            ("country", "Country"),
            ("company|org", "Company"),
            ("password", "Password"),
            ("confirm.?password", "Confirm Password"),
            ("username|user.?name", "Username"),
            ("card.?number|cc.?num", "Card Number"),
            ("expir|exp.?date", "Expiration"),
            ("cvv|cvc|security.?code", "CVV"),
            ("dob|birth.?date|date.?of.?birth", "Date of Birth"),
            ("ssn|social", "SSN"),
            ("comment|message|note", "Comments"),
        ] {
            m.insert(pattern.to_string(), label.to_string());
        }
        m
    }

    /// Build an ordered [`FillPlan`] that maps supplied `data` to analysed
    /// form fields using name / label / placeholder / pattern matching.
    pub fn build_fill_plan(
        &self,
        analysis: &FormAnalysis,
        data: &HashMap<String, String>,
    ) -> FillPlan {
        let mut plan = FillPlan::new();

        for field in analysis.fillable_fields() {
            let value = self.match_field_value(field, data);
            if let Some(val) = value {
                let strategy = InputStrategy::for_field(field);
                plan.add_action(FillAction {
                    field_selector: field.selector.clone(),
                    strategy,
                    value: val,
                    delay_ms: self.config.typing_delay_ms,
                    verify_after: self.config.verify_fills,
                });
            }
        }

        if let Some(ref submit) = analysis.submit_selector {
            plan.submit_after = true;
            plan.submit_selector = Some(submit.clone());
        }

        plan
    }

    /// Try to find a value in `data` that corresponds to `field`, using
    /// several matching strategies in priority order.
    fn match_field_value(
        &self,
        field: &FormField,
        data: &HashMap<String, String>,
    ) -> Option<String> {
        // 1. Exact name match
        if let Some(v) = data.get(&field.name) {
            return Some(v.clone());
        }

        // 2. Case-insensitive name match
        let lower_name = field.name.to_lowercase();
        for (k, v) in data {
            if k.to_lowercase() == lower_name {
                return Some(v.clone());
            }
        }

        // 3. Label match
        if !field.label.is_empty() {
            let lower_label = field.label.to_lowercase();
            for (k, v) in data {
                if k.to_lowercase() == lower_label {
                    return Some(v.clone());
                }
            }
        }

        // 4. Placeholder match
        if !field.placeholder.is_empty() {
            let lower_ph = field.placeholder.to_lowercase();
            for (k, v) in data {
                if lower_ph.contains(&k.to_lowercase()) {
                    return Some(v.clone());
                }
            }
        }

        // 5. Pattern matching against field_matchers
        for (pattern, label) in &self.field_matchers {
            if let Ok(re) = regex::Regex::new(&format!("(?i){}", pattern)) {
                if re.is_match(&field.name) || re.is_match(&field.label) {
                    let lower_label = label.to_lowercase();
                    for (k, v) in data {
                        if k.to_lowercase() == lower_label {
                            return Some(v.clone());
                        }
                    }
                }
            }
        }

        None
    }

    /// Register an autocomplete interaction strategy for a given domain/site.
    pub fn set_autocomplete_strategy(&mut self, domain: &str, strategy: AutocompleteStrategy) {
        self.autocomplete_strategies
            .insert(domain.to_string(), strategy);
    }

    /// Retrieve the autocomplete strategy registered for a domain.
    pub fn get_autocomplete_strategy(&self, domain: &str) -> Option<&AutocompleteStrategy> {
        self.autocomplete_strategies.get(domain)
    }

    /// Record the outcome of a fill attempt for historical tracking.
    pub fn record_result(&mut self, result: FillResult) {
        self.fill_history.push(result);
    }

    /// Fraction of historical fills that succeeded (0.0 – 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.fill_history.is_empty() {
            return 0.0;
        }
        let successes = self.fill_history.iter().filter(|r| r.success).count();
        successes as f64 / self.fill_history.len() as f64
    }

    /// Total number of fill attempts recorded.
    pub fn total_fills(&self) -> usize {
        self.fill_history.len()
    }

    /// Borrow the current engine configuration.
    pub fn config(&self) -> &AutomationConfig {
        &self.config
    }
}

impl Default for FormAutomationEngine {
    fn default() -> Self {
        Self::new(AutomationConfig::default())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── FieldType::from_html_type ────────────────────────────────────────

    #[test]
    fn field_type_text() {
        assert_eq!(FieldType::from_html_type("text"), FieldType::Text);
    }

    #[test]
    fn field_type_email() {
        assert_eq!(FieldType::from_html_type("email"), FieldType::Email);
    }

    #[test]
    fn field_type_password() {
        assert_eq!(FieldType::from_html_type("password"), FieldType::Password);
    }

    #[test]
    fn field_type_number() {
        assert_eq!(FieldType::from_html_type("number"), FieldType::Number);
    }

    #[test]
    fn field_type_tel_maps_to_number() {
        assert_eq!(FieldType::from_html_type("tel"), FieldType::Number);
    }

    #[test]
    fn field_type_phone() {
        assert_eq!(FieldType::from_html_type("phone"), FieldType::Phone);
    }

    #[test]
    fn field_type_date_variants() {
        assert_eq!(FieldType::from_html_type("date"), FieldType::Date);
        assert_eq!(FieldType::from_html_type("datetime-local"), FieldType::Date);
        assert_eq!(FieldType::from_html_type("month"), FieldType::Date);
        assert_eq!(FieldType::from_html_type("week"), FieldType::Date);
        assert_eq!(FieldType::from_html_type("time"), FieldType::Date);
    }

    #[test]
    fn field_type_select_variants() {
        assert_eq!(FieldType::from_html_type("select"), FieldType::Select);
        assert_eq!(FieldType::from_html_type("select-one"), FieldType::Select);
        assert_eq!(
            FieldType::from_html_type("select-multiple"),
            FieldType::Select
        );
    }

    #[test]
    fn field_type_checkbox() {
        assert_eq!(FieldType::from_html_type("checkbox"), FieldType::Checkbox);
    }

    #[test]
    fn field_type_radio() {
        assert_eq!(FieldType::from_html_type("radio"), FieldType::Radio);
    }

    #[test]
    fn field_type_textarea() {
        assert_eq!(FieldType::from_html_type("textarea"), FieldType::Textarea);
    }

    #[test]
    fn field_type_file() {
        assert_eq!(FieldType::from_html_type("file"), FieldType::File);
    }

    #[test]
    fn field_type_hidden() {
        assert_eq!(FieldType::from_html_type("hidden"), FieldType::Hidden);
    }

    #[test]
    fn field_type_search() {
        assert_eq!(FieldType::from_html_type("search"), FieldType::Search);
    }

    #[test]
    fn field_type_url() {
        assert_eq!(FieldType::from_html_type("url"), FieldType::Url);
    }

    #[test]
    fn field_type_color() {
        assert_eq!(FieldType::from_html_type("color"), FieldType::Color);
    }

    #[test]
    fn field_type_range() {
        assert_eq!(FieldType::from_html_type("range"), FieldType::Range);
    }

    #[test]
    fn field_type_unknown() {
        assert_eq!(FieldType::from_html_type("foobar"), FieldType::Unknown);
    }

    #[test]
    fn field_type_case_insensitive() {
        assert_eq!(FieldType::from_html_type("TEXT"), FieldType::Text);
        assert_eq!(FieldType::from_html_type("Email"), FieldType::Email);
        assert_eq!(FieldType::from_html_type("PASSWORD"), FieldType::Password);
    }

    // ── FormField ────────────────────────────────────────────────────────

    #[test]
    fn form_field_is_empty_when_no_value() {
        let f = FormField::new("#name", FieldType::Text);
        assert!(f.is_empty());
    }

    #[test]
    fn form_field_not_empty_when_has_value() {
        let mut f = FormField::new("#name", FieldType::Text);
        f.value = "Alice".to_string();
        assert!(!f.is_empty());
    }

    #[test]
    fn form_field_hidden_not_fillable() {
        let f = FormField::new("#csrf", FieldType::Hidden);
        assert!(!f.is_fillable());
    }

    #[test]
    fn form_field_file_not_fillable() {
        let f = FormField::new("#upload", FieldType::File);
        assert!(!f.is_fillable());
    }

    #[test]
    fn form_field_text_is_fillable() {
        let f = FormField::new("#name", FieldType::Text);
        assert!(f.is_fillable());
    }

    #[test]
    fn form_field_checkbox_is_fillable() {
        let f = FormField::new("#agree", FieldType::Checkbox);
        assert!(f.is_fillable());
    }

    // ── InputStrategy ────────────────────────────────────────────────────

    #[test]
    fn strategy_select_option() {
        let f = FormField::new("#country", FieldType::Select);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::SelectOption);
    }

    #[test]
    fn strategy_click_checkbox() {
        let f = FormField::new("#agree", FieldType::Checkbox);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::ClickCheckbox);
    }

    #[test]
    fn strategy_click_radio() {
        let f = FormField::new("#gender", FieldType::Radio);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::ClickRadio);
    }

    #[test]
    fn strategy_set_date() {
        let f = FormField::new("#dob", FieldType::Date);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::SetDate);
    }

    #[test]
    fn strategy_upload_file() {
        let f = FormField::new("#resume", FieldType::File);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::UploadFile);
    }

    #[test]
    fn strategy_autocomplete_select() {
        let mut f = FormField::new("#city", FieldType::Text);
        f.autocomplete = Some("address-level2".to_string());
        assert_eq!(
            InputStrategy::for_field(&f),
            InputStrategy::AutocompleteSelect
        );
    }

    #[test]
    fn strategy_clear_and_type_when_has_value() {
        let mut f = FormField::new("#name", FieldType::Text);
        f.value = "old".to_string();
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::ClearAndType);
    }

    #[test]
    fn strategy_direct_type_default() {
        let f = FormField::new("#name", FieldType::Text);
        assert_eq!(InputStrategy::for_field(&f), InputStrategy::DirectType);
    }

    // ── FormAnalysis ─────────────────────────────────────────────────────

    fn sample_analysis() -> FormAnalysis {
        let mut a = FormAnalysis::new("#form");
        a.fields.push(FormField::new("#name", FieldType::Text));
        a.fields.push(FormField::new("#csrf", FieldType::Hidden));

        let mut req = FormField::new("#email", FieldType::Email);
        req.required = true;
        a.fields.push(req);

        let mut filled = FormField::new("#phone", FieldType::Number);
        filled.value = "555-1234".to_string();
        a.fields.push(filled);

        a.submit_selector = Some("#submit".to_string());
        a
    }

    #[test]
    fn analysis_fillable_fields_excludes_hidden() {
        let a = sample_analysis();
        let fillable = a.fillable_fields();
        assert_eq!(fillable.len(), 3); // name, email, phone (hidden excluded)
    }

    #[test]
    fn analysis_required_fields() {
        let a = sample_analysis();
        assert_eq!(a.required_fields().len(), 1);
    }

    #[test]
    fn analysis_empty_required_fields() {
        let a = sample_analysis();
        assert_eq!(a.empty_required_fields().len(), 1);
    }

    #[test]
    fn analysis_field_count() {
        let a = sample_analysis();
        assert_eq!(a.field_count(), 4);
    }

    #[test]
    fn analysis_completion_ratio_partial() {
        let a = sample_analysis();
        // 3 fillable, 1 filled => 1/3
        let ratio = a.completion_ratio();
        assert!((ratio - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn analysis_completion_ratio_empty_form() {
        let a = FormAnalysis::new("#empty");
        assert!((a.completion_ratio() - 1.0).abs() < 1e-9);
    }

    // ── FillPlan ─────────────────────────────────────────────────────────

    #[test]
    fn fill_plan_default_is_empty() {
        let plan = FillPlan::default();
        assert_eq!(plan.action_count(), 0);
        assert!(!plan.submit_after);
    }

    #[test]
    fn fill_plan_add_actions() {
        let mut plan = FillPlan::new();
        plan.add_action(FillAction {
            field_selector: "#name".to_string(),
            strategy: InputStrategy::DirectType,
            value: "Alice".to_string(),
            delay_ms: 50,
            verify_after: false,
        });
        plan.add_action(FillAction {
            field_selector: "#email".to_string(),
            strategy: InputStrategy::DirectType,
            value: "a@b.com".to_string(),
            delay_ms: 50,
            verify_after: true,
        });
        assert_eq!(plan.action_count(), 2);
    }

    // ── WizardState ──────────────────────────────────────────────────────

    #[test]
    fn wizard_add_step_returns_index() {
        let mut w = WizardState::new();
        assert_eq!(w.add_step("Personal Info"), 0);
        assert_eq!(w.add_step("Address"), 1);
        assert_eq!(w.add_step("Review"), 2);
        assert_eq!(w.step_count(), 3);
    }

    #[test]
    fn wizard_first_step_is_current() {
        let mut w = WizardState::new();
        w.add_step("Step 1");
        w.add_step("Step 2");
        assert_eq!(w.current().unwrap().status, WizardStepStatus::Current);
        assert_eq!(w.steps[1].status, WizardStepStatus::NotReached);
    }

    #[test]
    fn wizard_advance() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        assert!(w.advance());
        assert_eq!(w.current_step, 1);
        assert_eq!(w.steps[0].status, WizardStepStatus::Completed);
        assert_eq!(w.steps[1].status, WizardStepStatus::Current);
    }

    #[test]
    fn wizard_advance_at_end_returns_false() {
        let mut w = WizardState::new();
        w.add_step("Only");
        assert!(!w.advance());
    }

    #[test]
    fn wizard_go_back() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        w.advance();
        assert!(w.go_back());
        assert_eq!(w.current_step, 0);
        assert_eq!(w.steps[0].status, WizardStepStatus::Current);
        assert_eq!(w.steps[1].status, WizardStepStatus::NotReached);
    }

    #[test]
    fn wizard_go_back_at_start_returns_false() {
        let mut w = WizardState::new();
        w.add_step("Only");
        assert!(!w.go_back());
    }

    #[test]
    fn wizard_is_complete() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        assert!(!w.is_complete());
        w.advance(); // S1 -> Completed, S2 -> Current
        assert!(!w.is_complete());
        // Manually complete S2
        w.steps[1].status = WizardStepStatus::Completed;
        assert!(w.is_complete());
    }

    #[test]
    fn wizard_is_complete_with_skipped() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        w.steps[0].status = WizardStepStatus::Skipped;
        w.steps[1].status = WizardStepStatus::Completed;
        assert!(w.is_complete());
    }

    #[test]
    fn wizard_progress() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        w.add_step("S3");
        assert!((w.progress() - 0.0).abs() < 1e-9);
        w.advance();
        // S1 completed => 1/3
        assert!((w.progress() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn wizard_progress_empty() {
        let w = WizardState::new();
        assert!((w.progress() - 0.0).abs() < 1e-9);
    }

    // ── CalendarState ────────────────────────────────────────────────────

    #[test]
    fn calendar_months_until_same_month() {
        let c = CalendarState::new(); // Jan 2024
        assert_eq!(c.months_until(1, 2024), 0);
    }

    #[test]
    fn calendar_months_until_forward() {
        let c = CalendarState::new(); // Jan 2024
        assert_eq!(c.months_until(6, 2024), 5);
    }

    #[test]
    fn calendar_months_until_backward() {
        let mut c = CalendarState::new();
        c.current_month = 6;
        c.current_year = 2024;
        assert_eq!(c.months_until(1, 2024), -5);
    }

    #[test]
    fn calendar_months_until_across_years() {
        let c = CalendarState::new(); // Jan 2024
        assert_eq!(c.months_until(3, 2025), 14);
    }

    // ── FillResult ───────────────────────────────────────────────────────

    #[test]
    fn fill_result_success() {
        let r = FillResult::success(5, 120.0);
        assert!(r.success);
        assert_eq!(r.fields_filled, 5);
        assert_eq!(r.fields_failed, 0);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn fill_result_partial_no_failures_is_success() {
        let r = FillResult::partial(3, 0, vec![], 80.0);
        assert!(r.success);
    }

    #[test]
    fn fill_result_partial_with_failures() {
        let r = FillResult::partial(2, 1, vec!["timeout".to_string()], 200.0);
        assert!(!r.success);
        assert_eq!(r.fields_failed, 1);
        assert_eq!(r.errors.len(), 1);
    }

    // ── AutocompleteStrategy ─────────────────────────────────────────────

    #[test]
    fn autocomplete_strategy_variants() {
        // Just verify all variants are distinct and serializable
        let strategies = vec![
            AutocompleteStrategy::TypeAndWait,
            AutocompleteStrategy::TypeAndArrowDown,
            AutocompleteStrategy::TypeAndClick,
            AutocompleteStrategy::TypeAndTab,
        ];
        assert_eq!(strategies.len(), 4);
        assert_ne!(strategies[0], strategies[1]);
        assert_ne!(strategies[2], strategies[3]);
    }

    // ── FormAutomationEngine ─────────────────────────────────────────────

    #[test]
    fn engine_default_config() {
        let engine = FormAutomationEngine::default();
        assert_eq!(engine.config().typing_delay_ms, 50);
        assert_eq!(engine.config().action_delay_ms, 200);
        assert_eq!(engine.config().max_retries, 3);
        assert!(engine.config().verify_fills);
    }

    #[test]
    fn engine_build_fill_plan_exact_name_match() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        let mut field = FormField::new("#email", FieldType::Email);
        field.name = "email".to_string();
        analysis.fields.push(field);

        let mut data = HashMap::new();
        data.insert("email".to_string(), "test@example.com".to_string());

        let plan = engine.build_fill_plan(&analysis, &data);
        assert_eq!(plan.action_count(), 1);
        assert_eq!(plan.actions[0].value, "test@example.com");
    }

    #[test]
    fn engine_build_fill_plan_case_insensitive_match() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        let mut field = FormField::new("#email", FieldType::Email);
        field.name = "Email".to_string();
        analysis.fields.push(field);

        let mut data = HashMap::new();
        data.insert("email".to_string(), "ci@example.com".to_string());

        let plan = engine.build_fill_plan(&analysis, &data);
        assert_eq!(plan.action_count(), 1);
    }

    #[test]
    fn engine_build_fill_plan_label_match() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        let mut field = FormField::new("#f1", FieldType::Text);
        field.name = "field_123".to_string();
        field.label = "Full Name".to_string();
        analysis.fields.push(field);

        let mut data = HashMap::new();
        data.insert("Full Name".to_string(), "Alice Smith".to_string());

        let plan = engine.build_fill_plan(&analysis, &data);
        assert_eq!(plan.action_count(), 1);
        assert_eq!(plan.actions[0].value, "Alice Smith");
    }

    #[test]
    fn engine_build_fill_plan_no_match_skips_field() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        let mut field = FormField::new("#xyz", FieldType::Text);
        field.name = "xyz_unknown".to_string();
        analysis.fields.push(field);

        let mut data = HashMap::new();
        data.insert("email".to_string(), "a@b.com".to_string());

        let plan = engine.build_fill_plan(&analysis, &data);
        assert_eq!(plan.action_count(), 0);
    }

    #[test]
    fn engine_build_fill_plan_with_submit() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        analysis.submit_selector = Some("#go".to_string());

        let data = HashMap::new();
        let plan = engine.build_fill_plan(&analysis, &data);
        assert!(plan.submit_after);
        assert_eq!(plan.submit_selector.as_deref(), Some("#go"));
    }

    #[test]
    fn engine_build_fill_plan_skips_hidden_fields() {
        let engine = FormAutomationEngine::default();
        let mut analysis = FormAnalysis::new("#form");
        let mut hidden = FormField::new("#token", FieldType::Hidden);
        hidden.name = "token".to_string();
        analysis.fields.push(hidden);

        let mut data = HashMap::new();
        data.insert("token".to_string(), "abc123".to_string());

        let plan = engine.build_fill_plan(&analysis, &data);
        assert_eq!(plan.action_count(), 0);
    }

    #[test]
    fn engine_success_rate_no_history() {
        let engine = FormAutomationEngine::default();
        assert!((engine.success_rate() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn engine_success_rate_all_success() {
        let mut engine = FormAutomationEngine::default();
        engine.record_result(FillResult::success(3, 100.0));
        engine.record_result(FillResult::success(5, 200.0));
        assert!((engine.success_rate() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn engine_success_rate_mixed() {
        let mut engine = FormAutomationEngine::default();
        engine.record_result(FillResult::success(3, 100.0));
        engine.record_result(FillResult::partial(1, 2, vec!["err".into()], 50.0));
        assert!((engine.success_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn engine_total_fills() {
        let mut engine = FormAutomationEngine::default();
        assert_eq!(engine.total_fills(), 0);
        engine.record_result(FillResult::success(1, 10.0));
        assert_eq!(engine.total_fills(), 1);
    }

    #[test]
    fn engine_autocomplete_strategy_roundtrip() {
        let mut engine = FormAutomationEngine::default();
        engine.set_autocomplete_strategy("google.com", AutocompleteStrategy::TypeAndArrowDown);
        assert_eq!(
            engine.get_autocomplete_strategy("google.com"),
            Some(&AutocompleteStrategy::TypeAndArrowDown)
        );
        assert_eq!(engine.get_autocomplete_strategy("other.com"), None);
    }

    // ── Serialization round-trips ────────────────────────────────────────

    #[test]
    fn serde_roundtrip_field_type() {
        let ft = FieldType::Email;
        let json = serde_json::to_string(&ft).unwrap();
        let back: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ft);
    }

    #[test]
    fn serde_roundtrip_form_analysis() {
        let a = sample_analysis();
        let json = serde_json::to_string(&a).unwrap();
        let back: FormAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(back.field_count(), a.field_count());
        assert_eq!(back.form_selector, "#form");
    }

    #[test]
    fn serde_roundtrip_fill_plan() {
        let mut plan = FillPlan::new();
        plan.add_action(FillAction {
            field_selector: "#x".to_string(),
            strategy: InputStrategy::DirectType,
            value: "val".to_string(),
            delay_ms: 10,
            verify_after: false,
        });
        let json = serde_json::to_string(&plan).unwrap();
        let back: FillPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.action_count(), 1);
    }

    #[test]
    fn serde_roundtrip_wizard_state() {
        let mut w = WizardState::new();
        w.add_step("S1");
        w.add_step("S2");
        w.advance();
        let json = serde_json::to_string(&w).unwrap();
        let back: WizardState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.step_count(), 2);
        assert_eq!(back.current_step, 1);
    }

    #[test]
    fn serde_roundtrip_calendar_state() {
        let mut c = CalendarState::new();
        c.selected_date = Some("2024-06-15".to_string());
        let json = serde_json::to_string(&c).unwrap();
        let back: CalendarState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.selected_date.as_deref(), Some("2024-06-15"));
    }

    #[test]
    fn serde_roundtrip_fill_result() {
        let r = FillResult::partial(2, 1, vec!["oops".to_string()], 99.5);
        let json = serde_json::to_string(&r).unwrap();
        let back: FillResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fields_filled, 2);
        assert_eq!(back.fields_failed, 1);
        assert!(!back.success);
    }

    #[test]
    fn serde_roundtrip_autocomplete_strategy() {
        let s = AutocompleteStrategy::TypeAndClick;
        let json = serde_json::to_string(&s).unwrap();
        let back: AutocompleteStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
