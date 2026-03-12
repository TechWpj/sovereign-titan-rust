## Data Extraction Guidance

**Output Format:**
- Output strictly as JSON unless another format is explicitly requested.
- Use `null` for missing or unknown fields — never guess values.
- No commentary or explanations in the output — data only.

**Entity Recognition:**
- Extract named entities: people, organizations, locations, dates, amounts.
- Normalize formats: dates as ISO 8601, currencies with symbol and amount, phone numbers with country code.
- Preserve original text in a separate field when normalization is ambiguous.

**Structure:**
- Use consistent key naming (snake_case preferred).
- Arrays for multiple values of the same type.
- Nested objects only when the relationship is inherently hierarchical.

**Quality:**
- Flag low-confidence extractions with a `confidence` field.
- Preserve source context with a `source_text` field when helpful.
- Deduplicate repeated entities.
