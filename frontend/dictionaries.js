// Live per-source selection. Only the backend's acknowledged binding is displayed.
let sources = {};
let available = false;
let sessionId;
const pending = new Set();
const errors = new Map();
// Result of the user's own Reload. It is only true for the state it was made in,
// so it is dropped when the dictionary or the file changes, and after a few seconds.
const notes = new Map();
const NOTE_MS = 5000;

function setNote(source, text, status) {
    notes.set(source, {
        text, generation: status?.generation, id: status?.dictionary?.dictionary_id,
        until: Date.now() + NOTE_MS,
    });
    setTimeout(render, NOTE_MS + 50);
}

function activeNote(source, status, activeItem) {
    const note = notes.get(source);
    if (!note) return '';
    const valid = Date.now() < note.until && !activeItem?.changed_on_disk
        && note.generation === status.generation && note.id === status.dictionary?.dictionary_id;
    if (!valid) notes.delete(source);
    return valid ? note.text : '';
}

const basename = path => path.split(/[\\/]/).filter(Boolean).pop() || path;

// File name, plus the parent folder only when names would otherwise look identical.
function shortName(path, all) {
    const parts = path.split(/[\\/]/).filter(Boolean);
    const clash = all.filter(other => basename(other.path) === basename(path)).length > 1;
    return clash && parts.length > 1 ? parts.slice(-2).join('/') : basename(path);
}

const buildLabel = info => (info?.build_id != null ? `build ${info.build_id}` : '');

function activate(source, status, dictionaryId) {
    return control({
        type: 'dictionary.activate', source_id: source,
        dictionary_id: dictionaryId, expected_generation: status.generation,
        parser_type: status.parser_type,
    }).then(reply => reply.status);
}

const reload = (source, status) => control({
    type: 'dictionary.reload', source_id: source, expected_generation: status.generation,
});

function reloadMessage(reply) {
    return reply.reloaded ? `Reloaded${buildLabel(reply.dictionary) ? ` · ${buildLabel(reply.dictionary)}` : ''}` : 'Already up to date';
}

function createSelect(source) {
    const label = document.createElement('label');
    label.className = 'dictionary-choice';
    label.textContent = 'Dictionary ';
    const select = document.createElement('select');
    select.setAttribute('aria-label', `Dictionary for ${source}`);
    label.append(select);
    select.addEventListener('change', async () => {
        const current = sources[source].dictionary;
        const selected = select.value;
        pending.add(source);
        errors.delete(source);
        notes.delete(source);
        render(); // Restore the active value until the RX owner acknowledges.
        try {
            const applied = await activate(source, current, selected);
            sources[source].dictionary = applied;
        } catch (error) {
            errors.set(source, error.message);
        } finally {
            pending.delete(source);
            render();
            await refresh(); // Reconcile timeouts and changes by other clients.
        }
    });
    return label;
}

function el(tag, className, text) {
    const node = document.createElement(tag);
    node.className = className;
    if (text) node.textContent = text;
    return node;
}

function createRow(source) {
    const row = el('div', 'dictionary-control');
    const name = el('span', 'dictionary-name');
    const meta = el('span', 'dictionary-meta');
    const pill = el('span', 'dictionary-pill');
    pill.setAttribute('role', 'status');
    const button = el('button', 'dictionary-reload', '↻ Reload');
    button.type = 'button';
    button.addEventListener('click', async () => {
        const current = sources[source].dictionary;
        pending.add(source);
        errors.delete(source);
        notes.delete(source);
        render();
        try {
            const reply = await reload(source, current);
            sources[source].dictionary = reply.status;
            setNote(source, reloadMessage(reply), reply.status);
        } catch (error) {
            errors.set(source, error.message);
        } finally {
            pending.delete(source);
            render();
            await refresh();
        }
    });
    const error = el('span', 'dictionary-error');
    error.setAttribute('role', 'alert');
    row.append(name, meta, pill, button, error);
    return row;
}

function control(request) {
    return new Promise((resolve, reject) => {
        const url = new URL('/api/v1/control', location.href);
        url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const socket = new WebSocket(url);
        const id = 'dictionary-select';
        let settled = false;
        const finish = (error, result) => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            socket.close();
            if (error) reject(error); else resolve(result);
        };
        const timer = setTimeout(() => finish(new Error('Change not confirmed; checking live status.')), 15000);
        socket.onopen = () => socket.send(JSON.stringify({ ...request, id }));
        socket.onmessage = event => {
            let reply;
            try { reply = JSON.parse(event.data); } catch { return; }
            if (reply.id !== id) return;
            finish(reply.ok ? null : new Error(reply.error || 'Dictionary change rejected'), reply);
        };
        socket.onerror = () => finish(new Error('Dictionary control connection failed.'));
        socket.onclose = () => finish(new Error('Dictionary control connection closed.'));
    });
}

function render() {
    for (const pane of document.querySelectorAll('.pane')) {
        const source = pane.id.slice('pane-'.length);
        const status = sources[source]?.dictionary;
        let row = pane.querySelector('.dictionary-control');
        if (!status?.configured?.length) {
            if (row) row.remove();
            continue;
        }
        if (!row) {
            row = createRow(source);
            pane.insertBefore(row, pane.querySelector('.filter-bar'));
        }
        // A choice only exists with several configured dictionaries; Reload is always useful.
        const multiple = status.configured.length > 1;
        if (multiple && !row.querySelector('select')) row.prepend(createSelect(source));
        if (!multiple) row.querySelector('label')?.remove();
        const select = row.querySelector('select');
        const activeId = status.dictionary?.dictionary_id || '';
        const activeItem = status.configured.find(item => item.dictionary?.dictionary_id === activeId);
        const signature = JSON.stringify([status.configured, status.dictionary]);
        if (select && select.dataset.options !== signature) {
            select.replaceChildren();
            for (const item of status.configured) {
                const option = document.createElement('option');
                option.value = item.dictionary?.dictionary_id || '';
                const build = buildLabel(item.dictionary);
                option.textContent = shortName(item.path, status.configured)
                    + (build ? ` · ${build}` : '') + (item.error ? ' (unavailable)' : '');
                option.title = item.error || item.path;
                option.disabled = !!item.error;
                select.append(option);
            }
            if (!activeItem) {
                const option = document.createElement('option');
                option.value = activeId;
                option.textContent = activeId ? `Uploaded · ${activeId.slice(0, 8)}` : 'No active dictionary';
                select.append(option);
            }
            select.dataset.options = signature;
        }
        if (select) {
            select.value = activeId;
            select.disabled = !available || pending.has(source);
        }

        // Name and details of the dictionary that is decoding right now.
        const name = row.querySelector('.dictionary-name');
        name.hidden = multiple; // the selector already shows the name
        name.textContent = activeItem ? shortName(activeItem.path, status.configured)
            : (activeId ? 'Uploaded dictionary' : 'No dictionary');
        name.title = activeItem?.path || activeId;
        // The selector's options already carry the build; do not repeat it next to them.
        const details = [multiple ? '' : buildLabel(status.dictionary), activeId && `#${activeId.slice(0, 8)}`].filter(Boolean);
        const meta = row.querySelector('.dictionary-meta');
        meta.textContent = available ? details.join(' · ') : '';
        meta.title = activeId ? `${activeItem?.path || 'uploaded'}\nsha256 ${activeId}` : '';

        const button = row.querySelector('.dictionary-reload');
        const fromFile = !!activeItem;
        button.disabled = !available || pending.has(source) || !fromFile;
        button.title = fromFile
            ? 'Re-read the active dictionary file, e.g. after rebuilding the firmware'
            : 'The active dictionary was uploaded, not read from a file';

        // One state pill: what the user needs to know about the file right now.
        const changed = !!activeItem?.changed_on_disk && available && !pending.has(source);
        const note = pending.has(source) ? '' : activeNote(source, status, activeItem);
        let state = ['', ''];
        if (!available) state = ['unavailable', 'Status unavailable'];
        else if (pending.has(source)) state = ['busy', 'Updating…'];
        else if (changed) state = status.watch ? ['busy', 'File changed · reloading…'] : ['stale', 'File changed — reload to use it'];
        else if (note) state = ['ok', note];
        else if (activeItem && status.watch) state = ['auto', 'Auto-reload on'];
        const pill = row.querySelector('.dictionary-pill');
        pill.textContent = state[1];
        pill.dataset.state = state[0];
        pill.hidden = !state[1];
        button.classList.toggle('attention', changed && !status.watch);
        row.querySelector('.dictionary-error').textContent = errors.get(source)
            || (status.watch_error ? `Auto-reload failed: ${status.watch_error}` : '');
    }
}

async function refresh() {
    try {
        const response = await fetch('/api/v1/status', { cache: 'no-store' });
        if (!response.ok) throw new Error('Status unavailable');
        const status = await response.json();
        available = !!status.control_api;
        const incoming = status.sources || {};
        if (sessionId === status.session_id) {
            for (const [source, value] of Object.entries(incoming)) {
                if (value.dictionary?.generation < sources[source]?.dictionary?.generation) {
                    value.dictionary = sources[source].dictionary;
                }
            }
        }
        sessionId = status.session_id;
        sources = incoming;
    } catch {
        available = false;
    }
    render();
}

const container = document.getElementById('container');
if (container) new MutationObserver(render).observe(container, { childList: true });
const interval = setInterval(refresh, 3000);
window.addEventListener('pagehide', () => clearInterval(interval));
refresh();
