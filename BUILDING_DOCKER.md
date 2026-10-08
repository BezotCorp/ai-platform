# Building and Running bcaip with Docker

This guide covers building Docker images for bcaip CLI for production use, CI/CD pipelines, and local development.

## Quick Start

### Using Pre-built Images

The easiest way to use bcaip with Docker is to pull the pre-built image from GitHub Container Registry:

```bash
# Pull the latest image
docker pull ghcr.io/BezotCorp/ai-platform:latest

# Run bcaip CLI
docker run --rm ghcr.io/BezotCorp/ai-platform:latest --version

# Run with LLM configuration
docker run --rm \
  -e BCAIP_PROVIDER=openai \
  -e BCAIP_MODEL=gpt-4o \
  -e OPENAI_API_KEY=$OPENAI_API_KEY \
  ghcr.io/BezotCorp/ai-platform:latest run -t "Hello, world!"
```

## Building from Source

### Prerequisites

- Docker 20.10 or later
- Docker Buildx (for multi-platform builds)
- Git

### Build the Image

1. Clone the repository:

```bash
git clone https://github.com/BezotCorp/ai-platform.git
cd bcaip
```

1. Build the Docker image:

```bash
docker build -t bcaip:local .
```

The build process:

- Uses a multi-stage build to minimize final image size
- Compiles with optimizations (LTO, stripping, size optimization)
- Results in a ~340MB image containing the `bcaip` CLI binary

### Build Options

For a development build with debug symbols:

```bash
docker build --build-arg CARGO_PROFILE_RELEASE_STRIP=false -t bcaip:dev .
```

For multi-platform builds:

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t bcaip:multi .
```

## Running bcaip in Docker

### CLI Mode

Basic usage:

```bash
# Show help
docker run --rm bcaip:local --help

# Run a command
docker run --rm \
  -e BCAIP_PROVIDER=openai \
  -e BCAIP_MODEL=gpt-4o \
  -e OPENAI_API_KEY=$OPENAI_API_KEY \
  bcaip:local run -t "Explain Docker containers"
```

With volume mounts for file access:

```bash
docker run --rm \
  -v $(pwd):/workspace \
  -w /workspace \
  -e BCAIP_PROVIDER=openai \
  -e BCAIP_MODEL=gpt-4o \
  -e OPENAI_API_KEY=$OPENAI_API_KEY \
  bcaip:local run -t "Analyze the code in this directory"
```

Interactive session mode with Databricks:

```bash
docker run -it --rm \
  -e BCAIP_PROVIDER=databricks \
  -e BCAIP_MODEL=databricks-dbrx-instruct \
  -e DATABRICKS_HOST="$DATABRICKS_HOST" \
  -e DATABRICKS_TOKEN="$DATABRICKS_TOKEN" \
  bcaip:local session
```

### Docker Compose

Create a `docker-compose.yml`:

```yaml
version: '3.8'

services:
  bcaip:
    image: ghcr.io/BezotCorp/ai-platform:latest
    environment:
      - BCAIP_PROVIDER=${BCAIP_PROVIDER:-openai}
      - BCAIP_MODEL=${BCAIP_MODEL:-gpt-4o}
      - OPENAI_API_KEY=${OPENAI_API_KEY}
    volumes:
      - ./workspace:/workspace
      - bcaip-config:/home/bcaip/.config/bcaip
    working_dir: /workspace
    stdin_open: true
    tty: true

volumes:
  bcaip-config:
```

Run with:

```bash
docker-compose run --rm bcaip session
```

## Configuration

### Environment Variables

The Docker image accepts all standard bcaip environment variables:

- `BCAIP_PROVIDER`: LLM provider (openai, anthropic, google, etc.)
- `BCAIP_MODEL`: Model to use (gpt-4o, claude-sonnet-4, etc.)
- Provider-specific API keys (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)

### Persistent Configuration

Mount the configuration directory to persist settings:

```bash
docker run --rm \
  -v ~/.config/bcaip:/home/bcaip/.config/bcaip \
  bcaip:local configure
```

### Installing Additional Tools

The image runs as a non-root user by default. To install additional packages:

```bash
# Run as root to install packages
docker run --rm \
  -u root \
  --entrypoint bash \
  bcaip:local \
  -c "apt-get update && apt-get install -y vim && bcaip --version"

# Or create a custom Dockerfile
FROM ghcr.io/BezotCorp/ai-platform:latest
USER root
RUN apt-get update && apt-get install -y \
    vim \
    tmux \
    && rm -rf /var/lib/apt/lists/*
USER bcaip
```

## CI/CD Integration

### GitHub Actions

```yaml
jobs:
  analyze:
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/BezotCorp/ai-platform:latest
      env:
        BCAIP_PROVIDER: openai
        BCAIP_MODEL: gpt-4o
        OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
    steps:
      - uses: actions/checkout@v4
      - name: Run bcaip analysis
        run: |
          bcaip run -t "Review this codebase for security issues"
```

### GitLab CI

```yaml
analyze:
  image: ghcr.io/BezotCorp/ai-platform:latest
  variables:
    BCAIP_PROVIDER: openai
    BCAIP_MODEL: gpt-4o
  script:
    - bcaip run -t "Generate documentation for this project"
```

## Image Details

### Size and Optimization

- **Base image**: Debian Bookworm Slim (minimal runtime dependencies)
- **Final size**: ~340MB
- **Optimizations**: Link-Time Optimization (LTO), binary stripping, size optimization
- **Binary included**: `/usr/local/bin/bcaip` (32MB)

### Security

- Runs as non-root user `bcaip` (UID 1000)
- Minimal attack surface with only essential runtime dependencies
- Regular security updates via automated builds

### Included Tools

The image includes essential tools for bcaip operation:

- `git` - Version control operations
- `curl` - HTTP requests
- `ca-certificates` - SSL/TLS support
- Basic shell utilities

## Troubleshooting

### Permission Issues

If you encounter permission errors when mounting volumes:

```bash
# Ensure the mounted directory is accessible
docker run --rm \
  -v $(pwd):/workspace \
  -u $(id -u):$(id -g) \
  bcaip:local run -t "List files"
```

### API Key Issues

If API keys aren't being recognized:

1. Ensure environment variables are properly set
2. Check that quotes are handled correctly in your shell
3. Use `docker run --env-file .env` for multiple environment variables

### Network Issues

For accessing local services from within the container:

```bash
# Use host network mode
docker run --rm --network host bcaip:local
```

## Advanced Usage

### Custom Entrypoint

Override the default entrypoint for debugging:

```bash
docker run --rm -it --entrypoint bash bcaip:local
```

### Resource Limits

Set memory and CPU limits:

```bash
docker run --rm \
  --memory="2g" \
  --cpus="2" \
  bcaip:local
```

### Multi-stage Development

For development with hot reload:

```bash
# Mount source code
docker run --rm \
  -v $(pwd):/usr/src/bcaip \
  -w /usr/src/bcaip \
  rust:1.82-bookworm \
  cargo watch -x run
```

## Building for Production

For production deployments:

1. Use specific image tags instead of `latest`
2. Use secrets management for API keys
3. Set up logging and monitoring
4. Configure resource limits and auto-scaling

Example production Dockerfile:

```dockerfile
FROM ghcr.io/BezotCorp/ai-platform:v1.6.0
# Add any additional tools needed for your use case
USER root
RUN apt-get update && apt-get install -y your-tools && rm -rf /var/lib/apt/lists/*
USER bcaip
```

## Contributing

When contributing Docker-related changes:

1. Test builds on multiple platforms (amd64, arm64)
2. Verify image size remains reasonable
3. Update this documentation
4. Consider CI/CD implications
5. Test with various LLM providers

## Related Documentation

- [BCAIP in Docker Tutorial](documentation/docs/tutorials/bcaip-in-docker.md) - Step-by-step tutorial
- [Installation Guide](https://bcaip.bezotcorp.com/docs/getting-started/installation) - All installation methods
- [Configuration Guide](https://bcaip.bezotcorp.com/docs/guides/config-files) - Detailed configuration options
