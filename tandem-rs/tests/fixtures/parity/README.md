# Golden Parity Fixtures

These fixtures are captured directly from the Python implementation of tandem 1.5.0.
They represent the golden behavioral parity contract between Python tandem and Rust tandem.

## Maintenance Policy
DO NOT edit these fixture files directly by hand unless there is an authorized, justified behavior change.
To regenerate all fixtures from the Python implementation, run:

```bash
uv run --project tandem python ../tandem-rs/tools/dump_parity.py
```

Any modification to these files must be accompanied by an explicit rationale documented in the task or pull request.
