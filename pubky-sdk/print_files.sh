#!/bin/bash

# Array of directories to skip
skip_dirs=(./target ./nexus-webapi/benches ./nexus-webapi/tests ./nexus-webapi/tests ./examples ./pubky ./docs ./bindings)

# Build the find command with exclusion patterns
find_cmd="find ."

for dir in "${skip_dirs[@]}"; do
  find_cmd+=" -path $dir -prune -o"
done

# Add the file types to include and the actions to perform
find_cmd+=" \( -name '*.rs' -o -name '*.toml' -o -name '*.md' \) -print"

# Execute the constructed find command
eval $find_cmd | while read -r file; do
  # Print the path to the file
  echo "$file"
  echo '```'
  # Print the content of the file
  cat "$file"
  echo '```'
done
