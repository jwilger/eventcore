# Docker Hub Authentication Setup for CI

To avoid Docker Hub rate limits in GitHub Actions, you need to set up Docker Hub authentication secrets.

## Steps to Configure

1. **Create a Docker Hub Access Token**:
   - Log in to [Docker Hub](https://hub.docker.com/)
   - Go to Account Settings → Security
   - Click "New Access Token"
   - Give it a descriptive name (e.g., "eventcore-ci")
   - Copy the generated token (you won't be able to see it again)

2. **Add Secrets to GitHub Repository**:
   - Go to your repository on GitHub
   - Navigate to Settings → Secrets and variables → Actions
   - Click "New repository secret"
   - Add two secrets:
     - `DOCKERHUB_USERNAME`: Your Docker Hub username
     - `DOCKERHUB_TOKEN`: The access token you created in step 1

## Why This is Needed

Docker Hub enforces rate limits for anonymous users:
- Anonymous users: 100 pulls per 6 hours per IP
- Authenticated users: 200 pulls per 6 hours
- Pro/Team users: Unlimited

Our integration tests use testcontainers which pull PostgreSQL images from Docker Hub. Without authentication, CI can hit rate limits, especially when multiple jobs run in parallel.

## Alternative Solutions

If you don't want to set up Docker Hub authentication:

1. **Use GitHub Container Registry**: Mirror the postgres:16-alpine image to ghcr.io
2. **Skip integration tests in CI**: Set environment variable to skip tests that require Docker
3. **Use a different PostgreSQL image source**: Some registries don't have rate limits

## Notes

- The Docker login step has `continue-on-error: true` so CI won't fail if secrets aren't configured
- Login is skipped for pull requests to avoid exposing secrets to untrusted code
- The authentication is only added to jobs that run integration tests (test and coverage jobs)