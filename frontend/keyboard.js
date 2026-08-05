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
