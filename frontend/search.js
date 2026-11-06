import { isErasedConsoleLine } from './ansi.js';

function phraseRegex(phrase) {
    return new RegExp(phrase.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu');
}

function textMatches(text, phrase) {
    return phrase ? [...text.matchAll(phraseRegex(phrase))].map(match => ({ start: match.index, end: match.index + match[0].length })) : [];
}

// Keep only offsets, not materialized line objects; live appends scan the new suffix.
export class PhraseSearch {
    constructor(phrase) {
        this.phrase = phrase;
        this.rows = null;
        this.scanned = 0;
        this.matches = [];
        this.cursor = -1;
        this.loading = false;
        this.error = '';
    }

    update(rows) {
        if (this.rows !== rows || rows.length < this.scanned) {
            this.rows = rows;
            this.scanned = 0;
            this.matches = [];
            this.cursor = -1;
        }
        if (!this.phrase) { this.scanned = rows.length; return; }
        for (let row = this.scanned; row < rows.length; row++) {
            const entry = rows[row];
            const raw = Array.isArray(entry) ? entry[1] : entry.rawText;
            const isTx = Array.isArray(entry) ? entry[2] : entry.isTx;
            if (!isTx && isErasedConsoleLine(raw)) continue;
            const text = raw.replace(/\x1b(?:\[[0-9;]*[A-Za-z]|\][^\x07]*\x07|[^[\]])/g, '');
            for (const match of textMatches(text, this.phrase)) this.matches.push({ row, ...match });
        }
        this.scanned = rows.length;
    }

    move(direction) {
        if (!this.matches.length) return null;
        this.cursor = this.cursor < 0
            ? (direction > 0 ? 0 : this.matches.length - 1)
            : (this.cursor + direction + this.matches.length) % this.matches.length;
        return this.matches[this.cursor];
    }
}

// Mark text nodes, never HTML strings: matches cannot corrupt ANSI spans or become markup.
export function markPhraseMatches(root, phrase, activeStart = null) {
    if (!phrase) return;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let text = '', node;
    while ((node = walker.nextNode())) {
        if (node.parentElement.closest('.ts')) continue;
        nodes.push({ node, start: text.length });
        text += node.textContent;
    }
    const matches = textMatches(text, phrase);
    for (const { node: textNode, start } of nodes) {
        const value = textNode.textContent;
        const overlaps = matches.filter(match => match.start < start + value.length && match.end > start);
        if (!overlaps.length) continue;
        const fragment = document.createDocumentFragment();
        let offset = 0;
        for (const match of overlaps) {
            const from = Math.max(0, match.start - start);
            const to = Math.min(value.length, match.end - start);
            fragment.append(document.createTextNode(value.slice(offset, from)));
            const mark = document.createElement('mark');
            mark.className = 'search-hit' + (match.start === activeStart ? ' search-current' : '');
            mark.textContent = value.slice(from, to);
            fragment.append(mark);
            offset = to;
        }
        fragment.append(document.createTextNode(value.slice(offset)));
        textNode.replaceWith(fragment);
    }
}
