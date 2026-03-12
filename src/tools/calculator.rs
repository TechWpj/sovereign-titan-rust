//! Calculator Tool — recursive descent parser for math expressions.
//!
//! Supports: +, -, *, /, ^, %, parentheses, and math functions.
//! Functions: sqrt, sin, cos, tan, log, ln, abs, ceil, floor, round.
//! Constants: pi, e.
//! Pure Rust, no external deps.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

pub struct CalculatorTool;

// ─────────────────────────────────────────────────────────────────────────────
// Recursive descent parser
// ─────────────────────────────────────────────────────────────────────────────

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.advance();
        }
    }

    fn parse(&mut self) -> Result<f64> {
        let result = self.expression()?;
        self.skip_whitespace();
        if self.pos < self.chars.len() {
            anyhow::bail!(
                "Unexpected character '{}' at position {}",
                self.chars[self.pos],
                self.pos
            );
        }
        Ok(result)
    }

    // expression = term (('+' | '-') term)*
    fn expression(&mut self) -> Result<f64> {
        let mut left = self.term()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('+') => {
                    self.advance();
                    left += self.term()?;
                }
                Some('-') => {
                    self.advance();
                    left -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // term = power (('*' | '/' | '%') power)*
    fn term(&mut self) -> Result<f64> {
        let mut left = self.power()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('*') => {
                    self.advance();
                    left *= self.power()?;
                }
                Some('/') => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        anyhow::bail!("Division by zero");
                    }
                    left /= right;
                }
                Some('%') => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        anyhow::bail!("Modulo by zero");
                    }
                    left %= right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // power = unary ('^' power)?
    fn power(&mut self) -> Result<f64> {
        let base = self.unary()?;
        self.skip_whitespace();
        if self.peek() == Some('^') {
            self.advance();
            let exp = self.power()?; // right-associative
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    // unary = ('-' | '+') unary | atom
    fn unary(&mut self) -> Result<f64> {
        self.skip_whitespace();
        match self.peek() {
            Some('-') => {
                self.advance();
                Ok(-self.unary()?)
            }
            Some('+') => {
                self.advance();
                self.unary()
            }
            _ => self.atom(),
        }
    }

    // atom = number | '(' expression ')' | function '(' expression ')' | constant
    fn atom(&mut self) -> Result<f64> {
        self.skip_whitespace();

        // Parenthesized expression
        if self.peek() == Some('(') {
            self.advance();
            let val = self.expression()?;
            self.skip_whitespace();
            if self.peek() != Some(')') {
                anyhow::bail!("Expected ')' at position {}", self.pos);
            }
            self.advance();
            return Ok(val);
        }

        // Try identifier (function or constant)
        if self.peek().is_some_and(|c| c.is_alphabetic()) {
            let start = self.pos;
            while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                self.advance();
            }
            let ident: String = self.chars[start..self.pos].iter().collect();
            let ident_lower = ident.to_lowercase();

            // Constants
            match ident_lower.as_str() {
                "pi" => return Ok(std::f64::consts::PI),
                "e" => return Ok(std::f64::consts::E),
                "tau" => return Ok(std::f64::consts::TAU),
                _ => {}
            }

            // Functions — expect '(' after name
            self.skip_whitespace();
            if self.peek() != Some('(') {
                anyhow::bail!("Unknown identifier: '{ident}'");
            }
            self.advance();
            let arg = self.expression()?;
            self.skip_whitespace();
            if self.peek() != Some(')') {
                anyhow::bail!("Expected ')' after function argument");
            }
            self.advance();

            return match ident_lower.as_str() {
                "sqrt" => {
                    if arg < 0.0 {
                        anyhow::bail!("sqrt of negative number");
                    }
                    Ok(arg.sqrt())
                }
                "sin" => Ok(arg.sin()),
                "cos" => Ok(arg.cos()),
                "tan" => Ok(arg.tan()),
                "asin" => Ok(arg.asin()),
                "acos" => Ok(arg.acos()),
                "atan" => Ok(arg.atan()),
                "log" | "log10" => {
                    if arg <= 0.0 {
                        anyhow::bail!("log of non-positive number");
                    }
                    Ok(arg.log10())
                }
                "ln" => {
                    if arg <= 0.0 {
                        anyhow::bail!("ln of non-positive number");
                    }
                    Ok(arg.ln())
                }
                "log2" => {
                    if arg <= 0.0 {
                        anyhow::bail!("log2 of non-positive number");
                    }
                    Ok(arg.log2())
                }
                "abs" => Ok(arg.abs()),
                "ceil" => Ok(arg.ceil()),
                "floor" => Ok(arg.floor()),
                "round" => Ok(arg.round()),
                "exp" => Ok(arg.exp()),
                _ => anyhow::bail!("Unknown function: '{ident}'"),
            };
        }

        // Number (integer or float)
        if self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            let start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                self.advance();
            }
            let num_str: String = self.chars[start..self.pos].iter().collect();
            let val: f64 = num_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid number: '{num_str}'"))?;
            return Ok(val);
        }

        anyhow::bail!(
            "Unexpected character '{}' at position {}",
            self.peek().unwrap_or('?'),
            self.pos
        )
    }
}

fn evaluate(expr: &str) -> Result<f64> {
    let cleaned = expr
        .replace("×", "*")
        .replace("÷", "/")
        .replace("**", "^");
    let mut parser = Parser::new(&cleaned);
    parser.parse()
}

fn format_result(val: f64) -> String {
    if val.is_nan() {
        "NaN".to_string()
    } else if val.is_infinite() {
        if val > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else if val == val.trunc() && val.abs() < 1e15 {
        format!("{}", val as i64)
    } else {
        // Up to 10 decimal places, trimming trailing zeros
        let s = format!("{:.10}", val);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

#[async_trait::async_trait]
impl super::Tool for CalculatorTool {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn description(&self) -> &'static str {
        "Evaluate mathematical expressions. Input: {\"action\": \"evaluate\", \"expression\": \"2 + 3 * 4\"}. \
         Supports: +, -, *, /, ^, %, parentheses. \
         Functions: sqrt, sin, cos, tan, log, ln, abs, ceil, floor, round, exp. \
         Constants: pi, e, tau."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let expr = input
            .get("expression")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("expr"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if expr.is_empty() {
            return Ok("calculator requires an \"expression\" field.".to_string());
        }

        info!("calculator: evaluating '{expr}'");

        match evaluate(expr) {
            Ok(val) => Ok(format!("{expr} = {}", format_result(val))),
            Err(e) => Ok(format!("Error evaluating '{expr}': {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    fn eval(expr: &str) -> f64 {
        evaluate(expr).unwrap()
    }

    #[test]
    fn test_basic_addition() {
        assert!((eval("2 + 3") - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_basic_subtraction() {
        assert!((eval("10 - 4") - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_multiplication() {
        assert!((eval("3 * 7") - 21.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_division() {
        assert!((eval("15 / 3") - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_division_by_zero() {
        assert!(evaluate("1 / 0").is_err());
    }

    #[test]
    fn test_modulo() {
        assert!((eval("17 % 5") - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_power() {
        assert!((eval("2 ^ 10") - 1024.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_power_right_associative() {
        // 2^3^2 = 2^(3^2) = 2^9 = 512
        assert!((eval("2 ^ 3 ^ 2") - 512.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parentheses() {
        assert!((eval("(2 + 3) * 4") - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_nested_parentheses() {
        assert!((eval("((2 + 3) * (4 - 1))") - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_order_of_operations() {
        assert!((eval("2 + 3 * 4") - 14.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_negative_number() {
        assert!((eval("-5 + 3") - (-2.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sqrt() {
        assert!((eval("sqrt(144)") - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sqrt_negative() {
        assert!(evaluate("sqrt(-1)").is_err());
    }

    #[test]
    fn test_sin() {
        assert!((eval("sin(0)") - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_cos() {
        assert!((eval("cos(0)") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_pi_constant() {
        assert!((eval("pi") - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_e_constant() {
        assert!((eval("e") - std::f64::consts::E).abs() < 1e-10);
    }

    #[test]
    fn test_log10() {
        assert!((eval("log(100)") - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_ln() {
        assert!((eval("ln(e)") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_abs() {
        assert!((eval("abs(-42)") - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ceil() {
        assert!((eval("ceil(3.2)") - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_floor() {
        assert!((eval("floor(3.8)") - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_round() {
        assert!((eval("round(3.5)") - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_complex_expression() {
        // sqrt(3^2 + 4^2) = sqrt(9+16) = sqrt(25) = 5
        assert!((eval("sqrt(3^2 + 4^2)") - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_float_arithmetic() {
        assert!((eval("0.1 + 0.2") - 0.3).abs() < 1e-10);
    }

    #[test]
    fn test_format_result_integer() {
        assert_eq!(format_result(42.0), "42");
    }

    #[test]
    fn test_format_result_float() {
        let r = format_result(3.14159);
        assert!(r.starts_with("3.14159"));
    }

    #[test]
    fn test_unicode_operators() {
        assert!((evaluate("6 × 7").unwrap() - 42.0).abs() < f64::EPSILON);
        assert!((evaluate("15 ÷ 3").unwrap() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_double_star_power() {
        assert!((evaluate("2 ** 8").unwrap() - 256.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_tool_evaluate() {
        let tool = CalculatorTool;
        let result = tool
            .execute(json!({"expression": "2 + 2"}))
            .await
            .unwrap();
        assert!(result.contains("= 4"));
    }

    #[tokio::test]
    async fn test_tool_missing_expression() {
        let tool = CalculatorTool;
        let result = tool.execute(json!({})).await.unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_tool_error() {
        let tool = CalculatorTool;
        let result = tool
            .execute(json!({"expression": "1/0"}))
            .await
            .unwrap();
        assert!(result.contains("Error"));
    }
}
