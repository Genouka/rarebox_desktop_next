<?php
/**
 * Release create/edit dialog.
 *
 * @var string $dialog_id
 * @var string $csrf
 */

use function RareBox\e;

$platforms = ['windows', 'macos', 'linux', 'android', 'all'];
$channels = ['stable', 'beta', 'nightly'];
?>
<dialog id="<?= e($dialog_id) ?>" class="modal">
    <form data-ajax data-mode="release">
        <input type="hidden" name="_csrf" value="<?= e($csrf) ?>">
        <input type="hidden" name="action" value="release_create">
        <input type="hidden" name="id" value="0">

        <header class="modal-head">
            <h2 data-title>New release</h2>
            <button type="button" class="btn btn-ghost btn-sm" data-close>Close</button>
        </header>

        <div class="modal-body">
            <div class="row">
                <label>Version
                    <input name="version" placeholder="1.2.0" required>
                </label>
                <label>Channel
                    <select name="channel">
                        <?php foreach ($channels as $c): ?>
                            <option value="<?= e($c) ?>"><?= e($c) ?></option>
                        <?php endforeach; ?>
                    </select>
                </label>
                <label>Platform
                    <select name="platform">
                        <?php foreach ($platforms as $p): ?>
                            <option value="<?= e($p) ?>"><?= e($p) ?></option>
                        <?php endforeach; ?>
                    </select>
                </label>
            </div>

            <label>Download URL
                <input name="download_url" type="url" placeholder="https://…/RareBox-Desktop-1.2.0.exe" required>
            </label>

            <div class="row">
                <label>File size (bytes)
                    <input name="file_size" type="number" min="0" step="1" value="0">
                </label>
                <label>Minimum supported version
                    <input name="min_version" placeholder="optional, e.g. 0.9.0">
                </label>
                <label>Publish at
                    <input name="published_at" placeholder="YYYY-MM-DD HH:MM (future = scheduled)">
                </label>
            </div>

            <label class="check">
                <input type="checkbox" name="force_update" value="1">
                <span>Force update — clients on an older build cannot dismiss the prompt</span>
            </label>

            <label>Release notes
                <textarea name="notes" rows="5" placeholder="Shown in the app update dialog and on the website changelog."></textarea>
            </label>
        </div>

        <footer class="modal-foot">
            <span class="result" role="status"></span>
            <button type="button" class="btn" data-close>Cancel</button>
            <button type="submit" class="btn btn-primary">Save release</button>
        </footer>
    </form>
</dialog>
