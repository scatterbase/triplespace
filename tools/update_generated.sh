#!/bin/sh
# Bring a pull request's branch up to date when its generated files are what stands in the way
# (AGENTS.md, "Generated files"): run by .github/workflows/generated-files.yml, and by hand the same.
#
#   tools/update_generated.sh BRANCH [BASE]      BASE defaults to main
#
# Merges origin/BASE into origin/BRANCH in the current checkout and pushes the result only if it
# was needed and is clean:
#   - the merge conflicts only in generated files: tools/resolve_generated.py resolves them by
#     regenerating, and the merge commit is pushed;
#   - the merge is clean but leaves generated files stale (both sides added claims, ADR entries or
#     catalogue rows): they are regenerated into the merge commit, which is pushed;
#   - the branch already contains BASE and its own generated files are stale: one commit
#     regenerating them is pushed;
#   - otherwise (the merge is clean and current, or a conflict touches a source) nothing is
#     pushed; a conflict in a source is left for the author, and the run says which files.
# Never rebases or force-pushes: the push is a fast-forward of BRANCH, and fails harmlessly if
# the author pushed in the meantime. Leaves the checkout on BRANCH.
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
msg="Merge $base into $branch: generated files regenerated (tools/update_generated.sh)"

if git merge --no-ff --no-edit -q -m "$msg" "origin/$base" >/dev/null 2>&1; then
    conflicted=
else
    conflicted=1
    if [ -z "$(git diff --name-only --diff-filter=U)" ]; then
        git merge --abort 2>/dev/null || true
        echo "::warning::$branch: git could not merge $base (not a conflict); left for the author"
        exit 0
    fi
    if ! python3 "$resolve"; then
        git merge --abort
        echo "::notice::$branch: the merge with $base conflicts outside generated files; left for the author"
        exit 0
    fi
    git commit -q --no-edit -m "$msg"
fi

if [ -z "$conflicted" ]; then
    if docs/check.sh >/dev/null 2>&1; then
        git reset -q --hard "$start"
        echo "$branch: nothing to do (merges cleanly with $base and its generated files are current)"
        exit 0
    fi
    docs/check.sh --write >/dev/null 2>&1 || true
    if ! docs/check.sh >/dev/null 2>&1; then
        git reset -q --hard "$start"
        echo "::warning::$branch: the documentation checks fail for a reason regenerating does not fix; left for the author"
        exit 0
    fi
    git add -u -- docs
    if [ "$(git rev-parse HEAD)" = "$start" ]; then
        # BRANCH already contained BASE: no merge commit, so the regeneration is its own commit.
        git commit -q -m "Regenerate generated files (tools/update_generated.sh)"
    else
        git commit -q --amend --no-edit
    fi
fi

if git push -q origin "HEAD:refs/heads/$branch"; then
    echo "$branch: pushed $(git rev-parse --short HEAD)"
else
    echo "::warning::$branch: push refused (the branch moved meanwhile?); the next run will try again"
fi
