#!/usr/bin/env bash
# Setup GitHub branch protection rules for claude-monitor
# Run once: bash scripts/setup-branch-protection.sh
# Requires: gh CLI authenticated with admin access

set -euo pipefail

REPO="ThiagoEMatumoto/claude-monitor"
BRANCH="main"

echo "Setting up branch protection for $REPO ($BRANCH)..."

gh api -X PUT "repos/$REPO/branches/$BRANCH/protection" \
  --input - <<'EOF'
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "Rust checks",
      "Security audit",
      "Workflow change guard"
    ]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "required_approving_review_count": 1,
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "require_last_push_approval": true
  },
  "restrictions": null,
  "required_linear_history": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "block_creations": false,
  "required_conversation_resolution": true
}
EOF

echo "Branch protection configured."
echo ""
echo "Manual steps needed in GitHub Settings > Repository:"
echo "  1. Enable 'Require signed commits' (optional but recommended)"
echo "  2. Enable 'Private vulnerability reporting' in Security tab"
echo "  3. Add branch ruleset for tag protection (v* tags only by maintainers)"
echo ""
echo "Done."
