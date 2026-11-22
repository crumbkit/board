# Sample standard

Copy this file into your own repository or board and give it your team's name.
Rename the IDs to a prefix of your own, such as `TEAM-1`, and keep each ID unique.
Edit, add or remove rows, and give each row at least one pattern in inline code in its Where it applies cell.

## Rules

| ID | Rule | Where it applies | How to check |
| --- | --- | --- | --- |
| SAMPLE-1 | The README says how to build the project and how to run its tests. | `README*` | Look for a section or commands that build the project and run the tests. |
| SAMPLE-2 | One command runs every test. | `README*`, `Makefile`, `justfile`, `package.json`, `Cargo.toml`, `pyproject.toml`, `bin/**`, `scripts/**` | Find the command the README names and check that it runs the whole test suite. |
| SAMPLE-3 | The repository holds no secrets, such as passwords, API keys or private keys. | `**` | Search for private key headers, tokens with known prefixes and assignments to names such as `password` or `secret`. Name the file and line, never the value. |
| SAMPLE-4 | The repository states its licence. | `LICENSE*`, `LICENCE*`, `COPYING*`, `README*` | Look for a licence file, or a README section that names the licence. |
| SAMPLE-5 | A changelog or release notes say what each release changed. | `CHANGELOG*`, `CHANGES*`, `NEWS*`, `RELEASES*`, `docs/**` | Look for a file that lists releases with what changed in each. |
| SAMPLE-6 | CI runs the tests on every change. | `.github/workflows/**`, `.gitlab-ci.yml`, `.circleci/**`, `Jenkinsfile` | Find a CI job that runs the test command on pushes or pull requests. |
