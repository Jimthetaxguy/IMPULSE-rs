#!/bin/bash
# Impulse CLI - easier commands
# Source this file: source impulse.sh

alias cps='impulse-rs status'
alias cpi='impulse-rs init'
alias cpss='impulse-rs session-start'
alias cpss-n='impulse-rs session-start -n'
alias cpse='impulse-rs session-end'
alias cptw='impulse-rs track-write'
alias cptt='impulse-rs track-tool'
alias cph='impulse-rs history'
alias cpa='impulse-rs activity'
alias cpl='impulse-rs list-sessions'
alias cpc='impulse-rs config'
alias cplp='impulse-rs list-providers'

echo "Impulse aliases loaded. Commands:"
echo "  cps     - status"
echo "  cpi     - init"
echo "  cpss-n  - session-start -n <name> -p claude-code"
echo "  cpse    - session-end --session-id <id> --summary '...'"
echo "  cptw    - track-write --file <path> --session-id <id>"
echo "  cptt    - track-tool --tool <name> --session-id <id>"
echo "  cph     - history"
echo "  cpa     - activity"
echo "  cpl     - list-sessions"
echo "  cpc     - config"
echo "  cplp    - list-providers"
