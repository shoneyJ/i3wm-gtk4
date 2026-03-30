# Git Quick Help

## Stashing

```bash
git stash                          # stash all uncommitted changes
git stash push --staged -m "msg"   # stash only staged changes
git stash push -m "msg" -- file    # stash specific file(s)
git stash list                     # list all stashes
git stash show -p stash@{0}        # show stash diff
git stash pop                      # apply and remove latest stash
git stash apply stash@{1}          # apply without removing
git stash drop stash@{0}           # delete a stash
git stash clear                    # delete all stashes
```

## Staging

```bash
git add -p file                    # interactively stage hunks
git reset HEAD file                # unstage a file
git diff --cached                  # show staged diff
git diff --cached --name-only      # list staged files
```

## Branching

```bash
git branch                         # list local branches
git branch -a                      # list all branches (incl. remote)
git checkout -b new-branch         # create and switch to branch
git switch branch-name             # switch branches
git branch -d branch-name          # delete merged branch
git branch -D branch-name          # force delete branch
git merge branch-name              # merge branch into current
git rebase main                    # rebase current onto main
```

## Viewing History

```bash
git log --oneline -20              # compact recent history
git log --graph --oneline --all    # visual branch graph
git log --stat                     # show changed files per commit
git log -p file                    # history of a specific file
git blame file                     # line-by-line last change
git show commit-hash               # show a specific commit
git diff branch1..branch2          # diff between branches
```

## Undoing Changes

```bash
git checkout -- file               # discard unstaged changes to file
git restore file                   # same (modern syntax)
git reset --soft HEAD~1            # undo last commit, keep staged
git reset --mixed HEAD~1           # undo last commit, keep unstaged
git reset --hard HEAD~1            # undo last commit, discard changes
git revert commit-hash             # create new commit that undoes a commit
```

## Remotes

```bash
git remote -v                      # list remotes
git fetch --all                    # fetch all remotes
git pull --rebase                  # pull with rebase
git push -u origin branch          # push and set upstream
git push origin --delete branch    # delete remote branch
```

## Submodules

```bash
git submodule add url path         # add submodule
git submodule update --init        # init and fetch submodules
git submodule update --remote      # update to latest remote
git submodule status               # show submodule state
git rm path && git commit          # remove a submodule
```

## Cherry-Pick and Reflog

```bash
git cherry-pick commit-hash        # apply a commit to current branch
git reflog                         # history of HEAD movements
git checkout HEAD@{3}              # recover from reflog
```

## Tags

```bash
git tag v1.0                       # lightweight tag
git tag -a v1.0 -m "msg"          # annotated tag
git push origin v1.0               # push a tag
git push origin --tags             # push all tags
```

## Useful Aliases (add to ~/.gitconfig)

```ini
[alias]
    s  = status -sb
    lg = log --oneline --graph --all --decorate
    d  = diff
    dc = diff --cached
    co = checkout
    br = branch
    st = stash
```