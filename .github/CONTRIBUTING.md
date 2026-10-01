# Contributing to BCAIP

Thank you for contributing to BCAIP.

BCAIP is developed in the open, and contributions should follow the same workflow used for the project's own development.

## Branches

The repository uses the following branch structure:

- `main` is the stable branch.
- `dev` is the integration branch.
- Development work is done on dedicated working branches.

Do not develop directly on `main` or `dev`.

## Development workflow

1. Start from the current `dev` branch.
2. Create a dedicated branch for the change.
3. Keep the change focused on a coherent purpose.
4. Verify the affected code using the checks appropriate for that part of the project.
5. Push the working branch.
6. Open a pull request targeting `dev`.
7. Address review feedback and verification failures before integration.

Changes reach `main` through the project's integration and release process rather than directly from individual development branches.

## Pull requests

A pull request should:

- have a clear and focused purpose;
- avoid unrelated changes;
- explain what changed and why;
- identify relevant limitations or remaining work;
- include or update tests when the change requires them;
- keep documentation consistent with user-visible behavior.

Large changes should remain reviewable. Prefer coherent incremental work over unrelated changes grouped into a single pull request.

## Code quality

Follow the conventions already used by the part of the repository you are modifying.

Do not introduce a second implementation of an existing mechanism when the existing architecture can be extended.

Before submitting a pull request, run the checks relevant to the affected code. Depending on the component, this may include compilation, tests, formatting, linting, or static analysis.

Do not modify generated files manually when the repository provides a generation process for them.

## AI-assisted contributions

AI tools may be used during development.

The contributor remains responsible for understanding, reviewing, and validating the resulting changes. AI-generated code is held to the same standards as any other contribution.

## Licensing

By contributing to this repository, you agree that your contribution will be distributed under the licensing terms applicable to the files and components you modify.
