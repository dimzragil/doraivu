import re

with open("src/handlers.rs", "r") as f:
    code = f.read()

# Fix `token = new_token;` to `*token = new_token;`
code = code.replace("token = new_token;", "*token = new_token;")

# Add use crate::app;
code = code.replace("use crate::app::{Action, App, Event};", "use crate::app::{self, Action, App, Event};")

# Add missing functions
code = code.replace("use crate::{api, auth, download, trash, ui, upload};", "use crate::{api, auth, download, trash, ui, upload};\nuse crate::api::{fetch_files, fetch_quota, upload_file};\nuse crate::trigger_preview;")

with open("src/handlers.rs", "w") as f:
    f.write(code)
