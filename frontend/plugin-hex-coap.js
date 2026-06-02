(function () {
    const TYPE_NAMES = ['CON', 'NON', 'ACK', 'RST'];
    const REQUEST_CODES = {
        1: 'GET',
        2: 'POST',
        3: 'PUT',
        4: 'DELETE',
    };
    const RESPONSE_CODES = {
        65: '2.01 Created',
        66: '2.02 Deleted',
        67: '2.03 Valid',
        68: '2.04 Changed',
        69: '2.05 Content',
        95: '2.31 Continue',
        128: '4.00 Bad Request',
        129: '4.01 Unauthorized',
        130: '4.02 Bad Option',
        131: '4.03 Forbidden',
        132: '4.04 Not Found',
        133: '4.05 Method Not Allowed',
        134: '4.06 Not Acceptable',
        140: '4.12 Precondition Failed',
        141: '4.13 Request Entity Too Large',
        143: '4.15 Unsupported Content-Format',
        160: '5.00 Internal Server Error',
        161: '5.01 Not Implemented',
        162: '5.02 Bad Gateway',
        163: '5.03 Service Unavailable',
        164: '5.04 Gateway Timeout',
        165: '5.05 Proxying Not Supported',
    };
    const CONTENT_FORMATS = {
        0: 'text/plain',
        40: 'application/link-format',
        41: 'application/xml',
        42: 'application/octet-stream',
        47: 'application/exi',
        50: 'application/json',
        60: 'application/cbor',
    };
    const OPTION_NAMES = {
        // RFC 7252 core
        3:  'Uri-Host',
        7:  'Uri-Port',
        11: 'Uri-Path',
        12: 'Content-Format',
        14: 'Max-Age',
        15: 'Uri-Query',
        17: 'Accept',
        35: 'Proxy-Uri',
        39: 'Proxy-Scheme',
        // RFC 7252 caching
        1:  'If-Match',
        4:  'ETag',
        5:  'If-None-Match',
        // RFC 7641 Observe
        6:  'Observe',
        // Location (RFC 7252)
        8:  'Location-Path',
        20: 'Location-Query',
        // Block-wise (RFC 7959)
        23: 'Block2',
        27: 'Block1',
        28: 'Size2',
        60: 'Size1',
        // OSCORE (RFC 8613)
        9:  'OSCORE',
        // Echo / Request-Tag (RFC 9175)
        252: 'Echo',
        292: 'Request-Tag',
        // No-Response (RFC 7967)
        258: 'No-Response',
    };

    function _cleanHexCandidate(text) {
        return text.replace(/[^0-9a-fA-F]/g, '');
    }

    function _hexToBytes(clean) {
        if (clean.length < 8 || clean.length % 2 !== 0) return null;
        const bytes = [];
        for (let i = 0; i < clean.length; i += 2) {
            bytes.push(parseInt(clean.slice(i, i + 2), 16));
        }
        return bytes;
    }

    function _findCandidates(rawText) {
        const matches = [];
        const separated = rawText.match(/[0-9A-Fa-f][0-9A-Fa-f\s:,_|.-]{7,}/g) || [];
        separated.forEach(match => matches.push(match));
        const compact = rawText.match(/\b[0-9a-fA-F]{8,}\b/g) || [];
        compact.forEach(match => matches.push(match));
        return [...new Set(matches)].sort((a, b) => b.length - a.length);
    }

    function _readExtended(bytes, index, nibble) {
        if (nibble < 13) return { value: nibble, index };
        if (nibble === 13) {
            if (index >= bytes.length) return null;
            return { value: bytes[index] + 13, index: index + 1 };
        }
        if (nibble === 14) {
            if (index + 1 >= bytes.length) return null;
            return { value: ((bytes[index] << 8) | bytes[index + 1]) + 269, index: index + 2 };
        }
        return null;
    }

    function _bytesToAscii(bytes) {
        let out = '';
        for (let i = 0; i < bytes.length; i += 1) {
            const value = bytes[i];
            out += value >= 32 && value <= 126 ? String.fromCharCode(value) : '.';
        }
        return out;
    }

    function _bytesToHex(bytes) {
        return bytes.map(value => value.toString(16).padStart(2, '0')).join(' ');
    }

    function _bytesToCompactHex(bytes) {
        return bytes.map(value => value.toString(16).padStart(2, '0')).join('');
    }

    function _uintFromBytes(valueBytes) {
        let acc = 0;
        for (let i = 0; i < valueBytes.length; i++) acc = (acc << 8) | valueBytes[i];
        return acc;
    }

    function _optionValue(number, valueBytes) {
        // Block1 / Block2 — decode NUM, M, SZX (RFC 7959)
        if (number === 23 || number === 27) {
            const block = _uintFromBytes(valueBytes);
            const szx = block & 0x7;
            const m = (block >> 3) & 0x1;
            const num = block >> 4;
            const size = 1 << (szx + 4);
            return `NUM=${num} M=${m} SZX=${szx} (${size}B block)`;
        }
        // Opaque options that read best as hex.
        if (number === 1 || number === 4 || number === 9) {
            return _bytesToHex(valueBytes);
        }
        if (number === 252 || number === 292) {
            return `0x${_bytesToCompactHex(valueBytes)}`;
        }
        // If-None-Match — typically empty
        if (number === 5) {
            return valueBytes.length ? _bytesToHex(valueBytes) : '(empty)';
        }
        // Numeric options: Observe, Uri-Port, Max-Age, Size1, Size2, No-Response
        if (number === 6 || number === 7 || number === 14 || number === 28 || number === 60 || number === 258) {
            return String(_uintFromBytes(valueBytes));
        }
        // Content-Format / Accept — number with name
        if (number === 12 || number === 17) {
            const cf = _uintFromBytes(valueBytes);
            return CONTENT_FORMATS[cf] ? `${cf} (${CONTENT_FORMATS[cf]})` : String(cf);
        }
        // String options: Uri-Host, Uri-Path, Uri-Query, Location-Path, Location-Query,
        //                Proxy-Uri, Proxy-Scheme
        return _bytesToAscii(valueBytes);
    }

    function _codeText(code) {
        if (REQUEST_CODES[code]) return REQUEST_CODES[code];
        if (RESPONSE_CODES[code]) return RESPONSE_CODES[code];
        const cls = code >> 5;
        const detail = code & 0x1f;
        return `${cls}.${String(detail).padStart(2, '0')}`;
    }

    function _inlineCodeText(code) {
        if (REQUEST_CODES[code]) return REQUEST_CODES[code];
        const cls = code >> 5;
        const detail = code & 0x1f;
        return `${cls}.${String(detail).padStart(2, '0')}`;
    }

    function _looksLikeCoapCode(code) {
        if (REQUEST_CODES[code]) return true;
        if (RESPONSE_CODES[code]) return true;
        const cls = code >> 5;
        return code === 0 || cls === 2 || cls === 4 || cls === 5;
    }

    function _parseCoap(bytes, startOffset, options) {
        if (!bytes || bytes.length < 4) return null;
        const first = bytes[0];
        const version = first >> 6;
        if (version !== 1) return null;
        const type = (first >> 4) & 0x03;
        const tokenLength = first & 0x0f;
        if (tokenLength > 8 || bytes.length < 4 + tokenLength) return null;

        const code = bytes[1];
        if (!_looksLikeCoapCode(code)) return null;
        const messageId = (bytes[2] << 8) | bytes[3];
        let index = 4;
        const token = bytes.slice(index, index + tokenLength);
        index += tokenLength;

        let optionNumber = 0;
        const parsedOptions = [];
        let payload = [];
        while (index < bytes.length) {
            const byte = bytes[index++];
            if (byte === 0xff) {
                payload = bytes.slice(index);
                break;
            }
            const deltaInfo = _readExtended(bytes, index, byte >> 4);
            if (!deltaInfo) return null;
            index = deltaInfo.index;
            const lengthInfo = _readExtended(bytes, index, byte & 0x0f);
            if (!lengthInfo) return null;
            index = lengthInfo.index;
            optionNumber += deltaInfo.value;
            if (index + lengthInfo.value > bytes.length) return null;
            const valueBytes = bytes.slice(index, index + lengthInfo.value);
            index += lengthInfo.value;
            parsedOptions.push({
                number: optionNumber,
                name: OPTION_NAMES[optionNumber] || `Option-${optionNumber}`,
                value: _optionValue(optionNumber, valueBytes),
            });
        }

        const path = parsedOptions.filter(opt => opt.number === 11).map(opt => opt.value).join('/');
        const query = parsedOptions.filter(opt => opt.number === 15).map(opt => opt.value).join('&');
        const uri = `${path ? '/' + path : ''}${query ? '?' + query : ''}` || '/';
        const codeText = _codeText(code);
        const isRequest = !!REQUEST_CODES[code];
        const summary = isRequest
            ? `${codeText} ${uri}`
            : `${codeText}${path || query ? ' ' + uri : ''}`;
        const tokenText = token.length ? _bytesToCompactHex(token) : '';
        const messageIdText = messageId.toString(16).padStart(4, '0');
        const optionsText = parsedOptions.length
            ? parsedOptions.map(opt => `${opt.name}:${opt.value}`).join(', ')
            : '(none)';
        const inlineText = `v:${version} t:${TYPE_NAMES[type] || 'UNKNOWN'} c:${_inlineCodeText(code)} i:${messageIdText} {${tokenText}} [${optionsText === '(none)' ? '' : optionsText}] :: data len ${payload.length}`;

        const details = [
            `Version: ${version}`,
            `Type: ${TYPE_NAMES[type] || 'UNKNOWN'}`,
            `Code: ${codeText}`,
            `Message ID: 0x${messageIdText}`,
            `Offset: byte ${startOffset}`,
            `Token: ${token.length ? tokenText : '(none)'}`,
            `Options: ${optionsText}`,
            `Data len: ${payload.length}`,
        ];

        let score = 0;
        if (isRequest) score += 100;
        if (path || query) score += 40;
        if (parsedOptions.length) score += Math.min(parsedOptions.length, 8);
        if (token.length) score += 2;
        if (payload.length) score += 1;

        return {
            label: 'CoAP',
            summary,
            details,
            inlineText: options?.allLogs ? inlineText : '',
            disableTooltip: options?.allLogs === true,
            filterText: `coap ${summary} ${inlineText}`,
            classNames: ['line-plugin-match', 'line-plugin-coap'],
            score,
        };
    }

    function _findCoapInCandidate(candidate, options) {
        const clean = _cleanHexCandidate(candidate);
        let best = null;
        for (let start = 0; start <= clean.length - 8; start += 2) {
            const bytes = _hexToBytes(clean.slice(start));
            const parsed = _parseCoap(bytes, start / 2, options);
            if (!parsed) continue;
            if (!best || parsed.score > best.score) {
                best = parsed;
            }
        }
        if (best) delete best.score;
        return best;
    }

    window.EmbedLogPlugins.register({
        apiVersion: 1,
        kind: 'line',
        name: 'hex-coap',
        displayName: 'CoAP',
        settings: [
            {
                key: 'allLogs',
                type: 'boolean',
                label: 'All logs',
                description: 'Render decoded CoAP summaries inline for matching lines in this pane.',
                defaultValue: false,
            },
        ],
        analyzeLine(ctx) {
            const candidates = _findCandidates(ctx.rawText || '');
            for (let i = 0; i < candidates.length; i += 1) {
                const parsed = _findCoapInCandidate(candidates[i], ctx.options || {});
                if (parsed) return parsed;
            }
            return null;
        },
    });
})();
