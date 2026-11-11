#!/usr/bin/env python3
# stand-in for the herdr CLI: lists from $FAKE_HERDR_STATE (JSON {"agents": [...], "workspaces": [...]}),
# focus commands appended to $FAKE_HERDR_LOG
import json
import os
import sys

args = sys.argv[1:]
state = json.load(open(os.environ["FAKE_HERDR_STATE"]))
if args == ["agent", "list"]:
    print(json.dumps({"id": "cli:agent:list", "result": {"agents": state["agents"]}}))
elif args == ["workspace", "list"]:
    print(json.dumps({"id": "cli:workspace:list", "result": {"type": "workspace_list", "workspaces": state["workspaces"]}}))
elif args[1:2] == ["focus"] and len(args) == 3:
    if args[0] == "agent" and args[2] not in {a["pane_id"] for a in state["agents"]}:
        sys.exit(1)
    with open(os.environ["FAKE_HERDR_LOG"], "a") as log:
        log.write(" ".join(args) + "\n")
else:
    sys.exit(2)
