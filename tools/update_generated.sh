#!/bin/sh
# Bring a pull request's branch up to date when its generated files are what stands in the way
# (AGENTS.md, "Generated files"): run by .github/workflows/generated-files.yml, and by hand the same.
#
#   tools/update_generated.sh BRANCH [BASE]      BASE defaults to main
#
# Rebases origin/BRANCH onto origin/BASE in the current checkout, and pushes only if that was
# needed and comes out clean:
#   - the rebase stops on conflicts only in generated files: at each stop
#     tools/resolve_generated.py takes either side and regenerates from that commit's sources;
#   - the rebase is clean but the result's generated files are stale (both sides added claims, ADR
#     entries or catalogue rows);
#   - the branch already contains BASE and its own generated files are stale.
# In every case the tip's generated files are then regenerated, as one commit "Regenerate
# generated files" if anything changed, and the documentation checks must pass. Otherwise
# (BRANCH rebases cleanly and its generated files stay current, a conflict touches a source, or
# the checks fail for another reason) nothing is pushed, and the run says why.
#
# A rebased branch is pushed with --force-with-lease against the commit fetched here, so a push
# the author made meanwhile is never overwritten: the push is refused and the next run tries
# again. The author's local copy then needs `git pull --rebase`. Leaves the checkout on BRANCH.
set -eu
branch=$1
base=${2:-main}
# The checkout below replaces tools/ with the branch's, which may predate these scripts, and sh
# reads a script as it runs: so both run from a copy taken first, in the repository's root.
if [ -z "${UPDATE_GENERATED_COPY:-}" ]; then
    cd "$(dirname "$0")/.."
    copy=$(mktemp -d)
    cp tools/update_generated.sh tools/resolve_generated.py "$copy/"
    UPDATE_GENERATED_COPY=$copy exec sh "$copy/update_generated.sh" "$@"
fi
resolve="$UPDATE_GENERATED_COPY/resolve_generated.py"
cd "$(git rev-parse --show-toplevel)"

git fetch --no-tags -q origin "$base" "$branch"
git checkout -q -B "$branch" "origin/$branch"
start=$(git rev-parse HEAD)

rebasing() {
    [ -d "$(git rev-parse --git-path rebase-merge)" ] || [ -d "$(git rev-parse --git-path rebase-apply)" ]
}
give_up() {   # level, message: put the branch back as fetched and stop without pushing
    if rebasing; then git rebase --abort; fi
    git reset -q --hard "$start"
    echo "::$1::$branch: $2"
    exit 0
}

conflicted=
if ! GIT_EDITOR=true git rebase -q "origin/$base" >/dev/null 2>&1; then
    conflicted=1
    stops=0
    while rebasing; do
        stops=$((stops + 1))
        [ "$stops" -le 500 ] || give_up warning "the rebase onto $base stopped too often; left for the author"
        if [ -z "$(git diff --name-only --diff-filter=U)" ]; then
            if git diff --cached --quiet; then
                # The commit became empty once its generated files were regenerated: drop it.
                GIT_EDITOR=true git rebase --skip >/dev/null 2>&1 || true
                continue
            fi
            give_up warning "the rebase onto $base stopped for a reason other than a conflict; left for the author"
        fi
        # Each stop is one of BRANCH's own commits, never checked on its own: regenerate without
        # requiring the checks to pass; the tip is checked below.
        python3 "$resolve" --no-check ||
            give_up notice "rebasing onto $base conflicts outside generated files; left for the author"
        GIT_EDITOR=true git rebase --continue >/dev/null 2>&1 || true
    done
fi

if [ -z "$conflicted" ] && docs/check.sh >/dev/null 2>&1; then
    git reset -q --hard "$start"
    echo "$branch: nothing to do (rebases cleanly onto $base and its generated files are current)"
    exit 0
fi
docs/check.sh --write >/dev/null 2>&1 || true
docs/check.sh >/dev/null 2>&1 ||
    give_up warning "the documentation checks fail for a reason regenerating does not fix; left for the author"
git add -u -- docs
if ! git diff --cached --quiet; then
    git commit -q -m "Regenerate generated files (tools/update_generated.sh)"
fi
if [ "$(git rev-parse HEAD)" = "$start" ]; then
    echo "$branch: nothing to push"
    exit 0
fi

if git merge-base --is-ancestor "$start" HEAD; then
    lease=
else
    lease="--force-with-lease=refs/heads/$branch:$start"
fi
# shellcheck disable=SC2086
if git push -q $lease origin "HEAD:refs/heads/$branch"; then
    echo "$branch: pushed $(git rev-parse --short HEAD)${lease:+ (rebased onto $base)}"
else
    echo "::warning::$branch: push refused (the branch moved meanwhile?); the next run will try again"
fi
