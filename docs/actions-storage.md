# Actions artifact retention

Published installers and update files stay in GitHub Releases. Maintenance never deletes releases, tags, source files, run logs or dependency caches.

After a build completes, and daily as a fallback, the maintenance workflow removes superseded artifacts from known build families. It keeps the latest completed output, the latest successful output for each platform, every open pull request's evidence, and active or unknown workflow outputs. New artifact families are preserved until explicitly added to the policy.

Package artifacts last seven days, with diagnostic screenshots limited to three days. Download the permanent installers from Releases.

Run `node scripts/prune-actions-storage.mjs` to preview the repository's cleanup. Add `--apply` to remove only the superseded artifacts. Both commands require `GITHUB_REPOSITORY` and an authenticated GitHub CLI. Review the workflow summary for the amount removed.
