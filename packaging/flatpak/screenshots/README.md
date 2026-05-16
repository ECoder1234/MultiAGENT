# Screenshots

`dev.multiagent.multiagent-main.png` is a local Linux/Xvfb capture of the
current MultiAGENT UI. It is useful as a handoff artifact, but it is not wired
into AppStream because Flathub requires screenshot images to be direct HTTPS
URLs in the MetaInfo file.

Before submitting to Flathub:

1. Capture polished Linux window screenshots on a real desktop session.
2. Upload them to `multiagent.dev` or a tagged/commit-pinned source repository.
3. Add the direct HTTPS image URLs to
   `packaging/shared/dev.multiagent.multiagent.metainfo.xml`:

   ```bash
   scripts/add_flathub_screenshots.py \
     --size 1200x800 \
     https://raw.githubusercontent.com/ECoder1234/multiagent-flathub-assets/766e775808480a3c9f01238b7ff854c398ac329b/screenshots/dev.multiagent.multiagent-main.png
   ```

4. Run `scripts/validate_flathub_submission.sh`.
