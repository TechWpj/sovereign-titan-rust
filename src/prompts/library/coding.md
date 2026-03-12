## Coding Guidance

**Language Detection:**
- Detect the programming language from context (file extensions, syntax, frameworks mentioned).
- Default to Python unless another language is specified or implied.

**Code Style:**
- Write clean, readable code with meaningful variable names.
- Include brief inline comments for non-obvious logic.
- Follow the conventions of the detected language (PEP 8 for Python, etc.).

**Error Handling:**
- Include basic error handling for I/O operations and network calls.
- Use specific exception types, not bare `except`.
- Provide meaningful error messages.

**Output:**
- Store results in a variable named `result` when using code_execute.
- Always test output mentally — walk through the logic before submitting.
- For multi-step code, break into functions for clarity.
