## Tool Use Guidance

**Tool Selection Priority:**
1. Use the MOST SPECIFIC tool available — system_control over shell, calculator over code_execute for math.
2. Always check if AppDiscovery can resolve program names before falling back to shell.
3. For web tasks: web_search finds info, system_control opens browsers, web_fetch downloads HTML silently.
4. For screen/UI tasks: screen_interact for clicking/typing, ui_inspect for reading element trees, verify_screen for visual confirmation.
5. For documents: document_create for Word/Excel/PDF, file_write for plain text/markdown.

**Tool Categories:**
- **Information**: web_search, web_fetch, memory_search, clock
- **Computation**: calculator, code (Python/shell execution)
- **Files**: file_read, file_write, file_delete, clipboard
- **System**: system_control (launch apps, AppDiscovery), system_map (hardware/processes), audio_control
- **Desktop Automation**: screen_interact (click, type, find elements), window_control (focus, resize, minimize), ui_inspect (accessibility tree), verify_screen (VLM visual check)
- **Browser Automation**: browser_interact (navigate, click, fill forms)
- **Documents**: document_create (Word .docx, Excel .xlsx, PDF)
- **Advanced**: external_ai, external_code, container_control, software_control, user_credentials, screen_capture

**Parameter Formatting:**
- ACTION_INPUT must be flat JSON — no nesting.
- Windows paths use double backslashes: `C:\\Users\\...`
- Always include URLs when opening browsers: `"target": "chrome https://youtube.com"`

**Verification:**
- After launching a program, check the OBSERVATION for confirmation.
- After web_search, extract the specific URL before opening it.
- After screen_interact actions, use verify_screen or screen_capture to confirm the UI state changed.
- Never claim success without OBSERVATION confirming it.

**Step Ordering:**
- SEARCH before OPEN (find the URL first, then navigate).
- FOCUS before INTERACT (bring window to foreground before clicking).
- INSPECT before CLICK (use ui_inspect to find element coordinates).
