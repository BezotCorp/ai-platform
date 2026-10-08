# @bezotcorp/bcaip-acp

Install and resolve the BCAIP executable through npm.

This package distributes the BCAIP CLI using platform-specific optional npm
dependencies. It does not contain or depend on the BCAIP ACP client.

## Installation

```bash
npm install @bezotcorp/bcaip-acp
```

The matching `@bezotcorp/bcaip-binary-*` package is installed automatically. Do not
install a platform package directly; `@bezotcorp/bcaip-acp` provides the supported
`bcaip` command.

## Usage

Run the BCAIP CLI installed by the package:

```bash
npx bcaip acp
npx bcaip serve
```

The launcher forwards arguments and standard input, output, and error streams to
the native executable. It preserves the executable's exit status and forwards
termination signals.

Resolve the executable path programmatically:

```typescript
import { resolveGooseBinary } from "@bezotcorp/bcaip-acp";

const binaryPath = resolveGooseBinary();
```

`resolveGooseBinary()` first uses `BCAIP_BINARY` when it is set. Otherwise, it
selects the package matching `process.platform` and `process.arch`. In both
cases it verifies that the executable exists and returns an absolute path.

Use the override to run a locally built or custom BCAIP executable:

```bash
BCAIP_BINARY=/path/to/bcaip npx bcaip acp
```

`BCAIP_BINARY` must point directly to a native BCAIP executable, not a
`node_modules/.bin/bcaip` command shim.

Supported platforms:

| Operating system | Architecture |
| ---------------- | ------------ |
| macOS            | ARM64        |
| macOS            | x64          |
| Linux            | ARM64        |
| Linux            | x64          |
| Windows          | x64          |

Package managers must install optional dependencies. If optional dependencies
are disabled, the resolver reports which platform package is missing.
