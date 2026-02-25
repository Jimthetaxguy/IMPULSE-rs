import os
import glob
import json

projects_dir = os.path.expanduser("~/.claude/projects")
jsonl_files = glob.glob(os.path.join(projects_dir, "**/*.jsonl"), recursive=True)

if not jsonl_files:
    print("No JSONL files found.")
    exit(0)

newest_file = max(jsonl_files, key=os.path.getmtime)
print(f"Newest file: {newest_file}")
print("-" * 40)

try:
    with open(newest_file, 'r') as f:
        lines = f.readlines()
        
        print(f"Total lines: {len(lines)}")
        print("Last 3 entries:")
        for line in lines[-3:]:
            data = json.loads(line)
            # Print keys and their types to understand the structure
            structure = {k: type(v).__name__ for k, v in data.items()}
            print(json.dumps(structure, indent=2))
            # If there's a predictable type/message structure, print it
            if 'message' in data and isinstance(data['message'], dict):
                print(f"  Message role: {data['message'].get('role')}")
                print(f"  Content length: {len(data['message'].get('content', []))}")
                
            print("-" * 20)
            
except Exception as e:
    print(f"Error: {e}")
