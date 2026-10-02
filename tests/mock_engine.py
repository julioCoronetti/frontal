import argparse
import json
import signal
import sys
import time


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input-format", required=True)
    parser.add_argument("--output-format", required=True)
    parser.add_argument("--conversation", default="mock-conversation-id")
    parser.add_argument("--project", default=None)
    parser.add_argument("--mode", default=None)
    parser.add_argument("--effort", default=None)
    args, _ = parser.parse_known_args()

    interrupted = False

    def handle_sigint(signum, frame):
        nonlocal interrupted
        interrupted = True

    signal.signal(signal.SIGINT, handle_sigint)

    init_event = {
        "event": "init",
        "conversation_id": args.conversation,
        "init": {
            "cwd": "/mock/cwd",
            "tools": ["mock_tool"],
            "permission_mode": "always-proceed",
        },
    }
    sys.stdout.write(json.dumps(init_event) + "\n")
    sys.stdout.flush()

    while True:
        line = sys.stdin.readline()
        if not line:
            break

        line = line.strip()
        if not line:
            continue

        try:
            payload = json.loads(line)
        except Exception:
            err_event = {
                "event": "result",
                "result": {
                    "conversation_id": args.conversation,
                    "status": "ERROR",
                    "error": "invalid json input",
                },
            }
            sys.stdout.write(json.dumps(err_event) + "\n")
            sys.stdout.flush()
            continue

        prompt = payload.get("message", {}).get("content", "")

        step0 = {
            "event": "step_update",
            "step_update": {
                "conversation_id": args.conversation,
                "step_index": 0,
                "state": "DONE",
                "step_type": "user_input",
            },
        }
        sys.stdout.write(json.dumps(step0) + "\n")
        sys.stdout.flush()

        thought = {
            "event": "thought",
            "thought": f"Analyzing mock prompt: {prompt}",
        }
        sys.stdout.write(json.dumps(thought) + "\n")
        sys.stdout.flush()

        if prompt == "SLOW":
            for _ in range(50):
                if interrupted:
                    break
                time.sleep(0.05)

        if interrupted:
            interrupted = False
            res = {
                "event": "result",
                "result": {
                    "conversation_id": args.conversation,
                    "status": "ERROR",
                    "error": "Turn interrupted by user",
                },
            }
            sys.stdout.write(json.dumps(res) + "\n")
            sys.stdout.flush()
            continue

        step1 = {
            "event": "step_update",
            "step_update": {
                "conversation_id": args.conversation,
                "step_index": 1,
                "state": "ACTIVE",
                "step_type": "agent_response",
                "text_delta": f"Response to: {prompt}",
            },
        }
        sys.stdout.write(json.dumps(step1) + "\n")
        sys.stdout.flush()

        step1_done = {
            "event": "step_update",
            "step_update": {
                "conversation_id": args.conversation,
                "step_index": 1,
                "state": "DONE",
                "step_type": "agent_response",
                "text_delta": "\n",
                "duration_seconds": 0.05,
            },
        }
        sys.stdout.write(json.dumps(step1_done) + "\n")
        sys.stdout.flush()

        result = {
            "event": "result",
            "result": {
                "conversation_id": args.conversation,
                "status": "SUCCESS",
                "response": f"Response to: {prompt}\n",
                "duration_seconds": 0.06,
            },
        }
        sys.stdout.write(json.dumps(result) + "\n")
        sys.stdout.flush()

    return 0


if __name__ == "__main__":
    sys.exit(main())
