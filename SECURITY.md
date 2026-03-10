# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| latest  | Yes       |

## Reporting a Vulnerability

**Do NOT open a public issue for security vulnerabilities.**

Instead, please report security issues via GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/ThiagoEMatumoto/claude-monitor/security/advisories)
2. Click "Report a vulnerability"
3. Provide a detailed description

You will receive a response within 72 hours. Please include:

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

## Scope

Security issues we care about:

- **Credential exposure**: OAuth tokens, API keys leaking via logs, crash dumps, or IPC
- **Supply chain**: Malicious or vulnerable dependencies
- **Local privilege escalation**: Tauri IPC abuse, command injection
- **Data exfiltration**: Unauthorized network requests, telemetry
- **Build tampering**: CI/CD pipeline manipulation

## Security Design Principles

1. **Credentials stay local**: OAuth tokens stored only in `~/.config/claude-monitor/credentials.json` with restrictive permissions
2. **Minimal network**: Only communicates with `api.anthropic.com` and `console.anthropic.com`
3. **No telemetry**: Zero data collection, everything stays on the user's machine
4. **Least privilege**: Tauri capability system restricts what the frontend can access
