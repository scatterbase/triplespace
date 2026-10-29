// The head script (ADR 0085 §6; 0086 §5): loaded blocking in <head>, after the palette's
// and type set's stylesheet links, under an immutable hashed name, which `script-src
// 'self'` permits. It marks the document as having JavaScript and, unless the server has
// written a signed-in reader's own choices (`data-appearance`), applies the choices this
// browser keeps before the first paint, pointing the stylesheet links at the reader's
// palette and type set where they chose others than the wiki's. It sets no cookie, so a
// public page stays public. It exposes the shared functions to the site's script, which
// loads later as a module and must not pull them into a chunk of its own.
import {
	apply, applySheets, read, readTheme, sheetChoices, write, source, clean, CHOICES, STORAGE_KEY
} from './appearance/core.js';

const root = document.documentElement;
root.classList.replace( 'client-nojs', 'client-js' );
window.tsAppearance = {
	apply, applySheets, read, readTheme, sheetChoices, write, source, clean, CHOICES, STORAGE_KEY
};
if ( !root.hasAttribute( 'data-appearance' ) ) {
	let storage;
	try {
		storage = window.localStorage;
	} catch ( e ) {
		storage = undefined;
	}
	const theme = readTheme( document );
	const stored = read( storage, sheetChoices( theme ), theme && theme.palette );
	if ( stored && Object.keys( stored ).length ) {
		apply( root, stored );
		applySheets( document, stored, theme );
	}
}
