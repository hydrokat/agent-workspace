# Golden Parity Fixtures

These fixtures are captured directly from the Python implementation of tandem 1.5.0.
They represent the golden behavioral parity contract between Python tandem and Rust tandem.

## Maintenance Policy
DO NOT edit these fixture files directly by hand unless there is an authorized, justified behavior change.
The Python implementation and its `dump_parity.py` generator were removed in the Rust cutover (p-012), so these fixtures are frozen.
Recover the generator from git history (the commit before the cutover) only for an authorized, justified behavior change.

Any modification to these files must be accompanied by an explicit rationale documented in the task or pull request.
