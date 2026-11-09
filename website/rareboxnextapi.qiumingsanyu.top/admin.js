/* RareBox admin: AJAX forms, edit dialogs, delete/toggle actions. */
(function () {
    'use strict';

    var meta = document.querySelector('meta[name="csrf"]');
    var CSRF = meta ? meta.getAttribute('content') : '';

    function setResult(el, ok, message) {
        if (!el) return;
        el.textContent = message || '';
        el.classList.toggle('err', !ok);
    }

    async function post(data) {
        var fd = data instanceof FormData ? data : new FormData();
        if (!(data instanceof FormData)) {
            Object.keys(data).forEach(function (k) { fd.set(k, data[k]); });
        }
        fd.set('_csrf', CSRF);
        var res = await fetch('/admin', {
            method: 'POST',
            headers: { 'X-Csrf-Token': CSRF, 'Accept': 'application/json' },
            body: fd,
            credentials: 'same-origin'
        });
        var body = {};
        try { body = await res.json(); } catch (e) { /* non-JSON error page */ }
        if (res.status === 401) {
            window.location.href = '/admin/login';
            throw new Error('signed_out');
        }
        return { ok: res.ok && body.ok === true, body: body, status: res.status };
    }

    /** FormData with checkboxes normalised to 0/1 (unchecked boxes send nothing). */
    function formData(form) {
        var fd = new FormData(form);
        form.querySelectorAll('input[type="checkbox"][name]').forEach(function (cb) {
            fd.set(cb.name, cb.checked ? '1' : '0');
        });
        return fd;
    }

    function errorText(res) {
        return (res.body && (res.body.message || res.body.error)) || ('HTTP ' + res.status);
    }

    /* ---------- Inline forms ---------- */
    document.querySelectorAll('form[data-ajax]').forEach(function (form) {
        var result = form.querySelector('.result');
        form.addEventListener('submit', async function (e) {
            e.preventDefault();
            var submit = form.querySelector('button[type="submit"]');
            if (submit) submit.disabled = true;
            setResult(result, true, 'Saving…');
            try {
                var res = await post(formData(form));
                if (res.ok) {
                    setResult(result, true, (res.body.message || 'Saved') + ' ✓');
                    var modal = form.closest('dialog');
                    if (modal) { setTimeout(function () { modal.close(); location.reload(); }, 450); }
                    else if (form.dataset.keep !== '1') { setTimeout(function () { location.reload(); }, 450); }
                    else { form.reset(); }
                } else {
                    setResult(result, false, errorText(res));
                }
            } catch (err) {
                if (err.message !== 'signed_out') setResult(result, false, 'Network error');
            } finally {
                if (submit) submit.disabled = false;
            }
        });
    });

    /* ---------- Delete ---------- */
    document.querySelectorAll('button[data-delete]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
            var kind = btn.dataset.delete;
            if (!confirm('Delete ' + kind + ' #' + btn.dataset.id + '? This cannot be undone.')) return;
            btn.disabled = true;
            var res = await post({ action: kind + '_delete', id: btn.dataset.id });
            if (res.ok) location.reload();
            else { btn.disabled = false; alert('Delete failed: ' + errorText(res)); }
        });
    });

    /* ---------- Publish / unpublish ---------- */
    document.querySelectorAll('button[data-toggle-announcement]').forEach(function (btn) {
        btn.addEventListener('click', async function () {
            var info = JSON.parse(btn.dataset.toggleAnnouncement);
            btn.disabled = true;
            var res = await post({
                action: 'announcement_update',
                id: info.id,
                active: info.active ? '0' : '1'
            });
            if (res.ok) location.reload();
            else { btn.disabled = false; alert('Update failed: ' + errorText(res)); }
        });
    });

    /* ---------- Dialogs ---------- */
    function openDialog(id, values, mode) {
        var dialog = document.getElementById(id);
        if (!dialog) return;
        var form = dialog.querySelector('form');
        form.reset();
        var title = dialog.querySelector('[data-title]');
        if (title) {
            title.textContent = values && values.id
                ? 'Edit ' + mode + ' #' + values.id
                : 'New ' + mode;
        }
        form.querySelector('[name="id"]').value = values && values.id ? values.id : '0';
        form.querySelector('[name="action"]').value = values && values.id ? mode + '_update' : mode + '_create';

        if (values) {
            Object.keys(values).forEach(function (key) {
                var field = form.querySelector('[name="' + key + '"]');
                if (!field) return;
                if (field.type === 'checkbox') field.checked = !!Number(values[key]);
                else field.value = values[key] === null || values[key] === undefined ? '' : values[key];
            });
            if (mode === 'release' && form.querySelector('[name="force_update"]')) {
                form.querySelector('[name="force_update"]').checked = !!Number(values.force_update);
            }
            if (mode === 'announcement' && form.querySelector('[name="active"]')) {
                form.querySelector('[name="active"]').checked = !!Number(values.active);
            }
        }
        var result = form.querySelector('.result');
        setResult(result, true, '');
        dialog.showModal();
    }

    document.querySelectorAll('[data-open]').forEach(function (btn) {
        btn.addEventListener('click', function () {
            openDialog(btn.dataset.open, null, btn.dataset.open.replace('-dialog', ''));
        });
    });

    document.querySelectorAll('button[data-edit-release]').forEach(function (btn) {
        btn.addEventListener('click', function () {
            openDialog('release-dialog', JSON.parse(btn.dataset.editRelease), 'release');
        });
    });

    document.querySelectorAll('button[data-edit-announcement]').forEach(function (btn) {
        btn.addEventListener('click', function () {
            openDialog('announcement-dialog', JSON.parse(btn.dataset.editAnnouncement), 'announcement');
        });
    });

    document.querySelectorAll('dialog [data-close]').forEach(function (btn) {
        btn.addEventListener('click', function () {
            var dialog = btn.closest('dialog');
            if (dialog) dialog.close();
        });
    });

    /* ---------- Session expiry watchdog ---------- */
    // Any 401 redirects above; nothing else to do here.
})();
