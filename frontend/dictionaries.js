// Live per-source selection. Only the backend's acknowledged binding is displayed.
let sources = {};
let available = false;
let sessionId;
const pending = new Set();
const errors = new Map();

function activate(source, status, dictionaryId) {
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
        socket.onopen = () => socket.send(JSON.stringify({
            type: 'dictionary.activate', id, source_id: source,
            dictionary_id: dictionaryId, expected_generation: status.generation,
            parser_type: status.parser_type,
        }));
        socket.onmessage = event => {
            let reply;
            try { reply = JSON.parse(event.data); } catch { return; }
            if (reply.id !== id) return;
            finish(reply.ok ? null : new Error(reply.error || 'Dictionary change rejected'), reply.status);
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
            row = document.createElement('div');
            row.className = 'dictionary-control';
            const label = document.createElement('label');
            label.textContent = 'Dictionary ';
            const select = document.createElement('select');
            select.setAttribute('aria-label', `Dictionary for ${source}`);
            label.append(select);
            const active = document.createElement('span');
            active.className = 'dictionary-active';
            const error = document.createElement('span');
            error.className = 'dictionary-error';
            error.setAttribute('role', 'alert');
            row.append(label, active, error);
            pane.insertBefore(row, pane.querySelector('.filter-bar'));
            select.addEventListener('change', async () => {
                const current = sources[source].dictionary;
                const selected = select.value;
                pending.add(source);
                errors.delete(source);
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
        }
        const select = row.querySelector('select');
        const activeId = status.dictionary?.dictionary_id || '';
        const signature = JSON.stringify([status.configured, status.dictionary]);
        if (select.dataset.options !== signature) {
            select.replaceChildren();
            for (const item of status.configured) {
                const option = document.createElement('option');
                option.value = item.dictionary?.dictionary_id || '';
                option.textContent = item.path + (item.error ? ' (unavailable)' : '');
                option.title = item.error || item.path;
                option.disabled = !!item.error;
                select.append(option);
            }
            if (!activeId || !status.configured.some(item => item.dictionary?.dictionary_id === activeId)) {
                const option = document.createElement('option');
                option.value = activeId;
                option.textContent = activeId ? `Uploaded dictionary (${activeId.slice(0, 12)})` : 'No active dictionary';
                select.append(option);
            }
            select.dataset.options = signature;
        }
        select.value = activeId;
        select.disabled = !available || pending.has(source);
        const active = row.querySelector('.dictionary-active');
        const activePath = status.configured.find(item => item.dictionary?.dictionary_id === activeId)?.path;
        active.textContent = pending.has(source) ? 'Switching…' : (available ? `Active: ${activePath || activeId || 'none'}` : 'Status unavailable');
        active.title = activePath || activeId;
        row.querySelector('.dictionary-error').textContent = errors.get(source) || '';
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
