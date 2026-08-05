// Shared keyboard-target helpers. Global viewer shortcuts must not consume
// normal editing input from filters, TX controls, marker fields, or plugins.
export function isEditableTarget(target) {
    return target instanceof HTMLInputElement
        || target instanceof HTMLTextAreaElement
        || target?.isContentEditable === true;
}

export function isComposingEvent(event) {
    return event.isComposing === true || event.keyCode === 229;
}

// Edge can retarget an auto-repeating key to an input after the pointer moves
// focus into it. Track where a key started so a key held before the click does
// not unexpectedly fill the newly focused control. Holding a key after focus
// is already in an input remains normal text-entry behavior.
const _keyOrigins = new Map();

document.addEventListener("keydown", event => {
    if (isComposingEvent(event)) return;
    const key = event.code || event.key;
    const origin = _keyOrigins.get(key);
    if (origin !== undefined) {
        if (isEditableTarget(event.target) && !isEditableTarget(origin)) {
            event.preventDefault();
            event.stopImmediatePropagation();
        }
        return;
    }
    _keyOrigins.set(key, event.target);
}, true);

document.addEventListener("keyup", event => {
    _keyOrigins.delete(event.code || event.key);
}, true);

window.addEventListener("blur", () => _keyOrigins.clear());
