# Agent instructions

Follow [CONTRIBUTING.md](CONTRIBUTING.md) for branching, PR requirements and CI, and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for where code belongs.

## Revbot review loop

Revbot reviews every pull request in this repository and is available as the `revbot` MCP server. After opening or updating a PR, follow the Revbot review loop:

1. Call `wait_for_review` with the repository (`alexanderwanyoike/vibez`), the PR number and the head SHA you pushed. Call it again while it reports the review is still running.
2. For each open finding, fix the problem, or dispute it with `reply_to_review` and concrete evidence. A suggested approach is optional; the problem is what must be addressed.
3. After pushing a fix, call `wait_for_review` again for the new head.
4. The loop is done when a completed review of the latest head reports no open findings. A failed or incomplete review is not a clean review; report it instead of treating the PR as done.
