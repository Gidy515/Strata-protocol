# Add this starter to your existing workspace

Copy `README.md` and the `docs/` directory into your existing `xau-carry-vault/` workspace root, beside `Anchor.toml`. This starter supplies documentation; your generated `programs/`, workspace configuration, and tests stay in place.

Create a GitHub repository named `strata-protocol`. If you plan to push your existing local workspace, create the GitHub repository empty without adding a README or gitignore there.

From your local workspace root, check your existing Git state and remote:

```bash
git status
git remote -v
```

If it is not yet a Git repository, run `git init`. Then commit the starter documentation:

```bash
git add README.md docs/
git commit -m "docs: add Strata architecture and project README"
git branch -M main
```

Connect the new repository using its actual URL:

```bash
git remote add origin https://github.com/YOUR_USERNAME/strata-protocol.git
git push -u origin main
```

If `origin` already exists, inspect it first and use `git remote set-url origin YOUR_REPOSITORY_URL` only if it should point to the new repository. Commit your generated Anchor scaffold separately after checking the gitignore and excluding private keys.
