import re

with open("src/main.rs", "r") as f:
    content = f.read()

# We want to extract the Event::Input handling and Event::Action handling.
# This might be tricky with regex due to nested braces.
