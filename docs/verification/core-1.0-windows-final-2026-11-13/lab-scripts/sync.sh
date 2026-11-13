#!/bin/bash
# Sync the working tree (tracked + untracked non-ignored) onto F:\ui4me\src
set -e
cd /home/toor/projects/ui4me-core
L=$(dirname "$0")
git ls-files -co --exclude-standard -z | grep -zv '^AGENTS.md$\|^CLAUDE.md$\|^LOCAL_FINISH_PROMPT.md$' | tar --null -T - -cf $L/sync.tar
scp -q $L/sync.tar administrator@omegaui:F:/ui4me/sync.tar
echo 'Set-Location F:\ui4me\src; Get-ChildItem -Force | Where-Object Name -notin @("target") | Remove-Item -Recurse -Force; tar -xf F:\ui4me\sync.tar; "tar exit $LASTEXITCODE"; (Get-ChildItem -Recurse -File -Force | Where-Object FullName -notlike "*\target\*" | Measure-Object).Count' | $L/wj sync 600 | tail -2
