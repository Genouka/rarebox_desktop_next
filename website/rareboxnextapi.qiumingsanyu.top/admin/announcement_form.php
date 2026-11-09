<?php
/**
 * Announcement create/edit dialog.
 *
 * @var string $dialog_id
 * @var string $csrf
 */

use function RareBox\e;

$levels = ['info', 'warning', 'critical'];
?>
<dialog id="<?= e($dialog_id) ?>" class="modal">
    <form data-ajax data-mode="announcement">
        <input type="hidden" name="_csrf" value="<?= e($csrf) ?>">
        <input type="hidden" name="action" value="announcement_create">
        <input type="hidden" name="id" value="0">

        <header class="modal-head">
            <h2 data-title>New announcement</h2>
            <button type="button" class="btn btn-ghost btn-sm" data-close>Close</button>
        </header>

        <div class="modal-body">
            <label>Title
                <input name="title" maxlength="200" placeholder="Maintenance window this Friday" required>
            </label>

            <div class="row">
                <label>Level
                    <select name="level">
                        <?php foreach ($levels as $l): ?>
                            <option value="<?= e($l) ?>"><?= e($l) ?></option>
                        <?php endforeach; ?>
                    </select>
                </label>
                <label>Starts at
                    <input name="starts_at" placeholder="empty = immediately">
                </label>
                <label>Ends at
                    <input name="ends_at" placeholder="empty = never">
                </label>
            </div>

            <label>Body
                <textarea name="body" rows="6" placeholder="Plain text. Line breaks are preserved."></textarea>
            </label>

            <label class="check">
                <input type="checkbox" name="active" value="1" checked>
                <span>Published — unchecked keeps it as a draft</span>
            </label>
        </div>

        <footer class="modal-foot">
            <span class="result" role="status"></span>
            <button type="button" class="btn" data-close>Cancel</button>
            <button type="submit" class="btn btn-primary">Save announcement</button>
        </footer>
    </form>
</dialog>
