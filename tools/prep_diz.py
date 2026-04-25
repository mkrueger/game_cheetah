import os
import sys
import tomllib

if len(sys.argv) != 2:
    print("need 1 argument")
    sys.exit(1)

with open(os.path.join("Cargo.toml"), "rb") as cargo:
    version = tomllib.load(cargo)["package"]["version"]

with open(os.path.join("build", "file_id.diz"), "r", encoding="utf-8") as file_id:
    lines = file_id.readlines()

new_lines = [line.replace("#VERSION", version) for line in lines]

with open(sys.argv[1], "w", encoding="utf-8") as f:
    f.writelines(new_lines)

print(version)
