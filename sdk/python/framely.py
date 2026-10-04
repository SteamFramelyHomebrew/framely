"""Framely backend protocol and lifecycle callbacks (Python 3, no dependencies)."""
import json
import os
import sys

def emit(event, data):
    print(json.dumps({"event": event, "data": data}), flush=True)

def serve(dispatch, lifecycle=None, visibility=None):
    """dispatch(method, params); lifecycle maps onInstall/onStart/... to callbacks.

    Independent hooks receive FRAMELY_LIFECYCLE_CONTEXT and exit once completed.
    Normal backend hooks are manager-only reserved protocol messages.
    visibility(snapshot) receives framely.ui.visibility when opted in via manifest.
    """
    lifecycle = lifecycle or {}
    phase = os.environ.get("FRAMELY_LIFECYCLE")
    if phase:
        try:
            callback = lifecycle.get(phase)
            if callback is None:
                raise ValueError("Lifecycle callback not registered: " + phase)
            callback(json.loads(os.environ["FRAMELY_LIFECYCLE_CONTEXT"]))
        except Exception as error:
            print(str(error), file=sys.stderr)
            raise SystemExit(1)
        return
    for line in sys.stdin:
        request = {}
        try:
            if len(line.encode()) > 65536:
                raise ValueError("Backend request too large")
            request = json.loads(line)
            method = request["method"]
            params = request.get("params") or {}
            if method.startswith("framely.lifecycle."):
                phase = {"framely.lifecycle.start": "onStart", "framely.lifecycle.stop": "onStop"}.get(method)
                if phase not in lifecycle:
                    raise ValueError("Lifecycle callback not registered")
                result = lifecycle[phase](params)
            elif method == "framely.ui.visibility" and visibility is not None:
                result = visibility(params)
            else:
                result = dispatch(method, params)
            response = {"id": request["id"], "result": result}
        except Exception as error:
            response = {"id": request.get("id"), "error": str(error)}
        print(json.dumps(response), flush=True)
