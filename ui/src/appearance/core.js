// The reader's appearance (ADR 0085 §6; 0086 §4–5; 19 §5.9), shared by the head script,
// which applies a stored choice before the first paint, and the Appearance menu, which
// applies a change at once: colour mode, contrast, text size and the colour-vision
// alternate (ADR 0094) as attributes on <html>, and the palette and type set as the
// stylesheets the page links to. The keys and their
// choices are preferences.toml's; the server's twin is triplespace-ui's appearance.rs.

/** @type {Object<string, string[]>} */
export const CHOICES = {
	'ui.mode': [ 'auto', 'light', 'dark' ],
	'ui.contrast': [ 'auto', 'more' ],
	'ui.text-size': [ 'small', 'medium', 'large', 'x-large' ],
	'ui.vision': [ 'none', 'by', 'rg' ]
};

/**
 * The retired palettes and what each reads as (ADR 0094 §4; themes.toml's retired
 * palettes, which a test holds this to): the palette that replaced it and the alternate.
 *
 * @type {Object<string, string[]>}
 */
export const RETIRED = {
	'parchment-by': [ 'parchment', 'by' ],
	'parchment-rg': [ 'parchment', 'rg' ]
};

/**
 * Stored choices with a retired palette read as what replaced it: the palette, or the
 * wiki's own where that is it, and the alternate where the reader chose none.
 *
 * @param {*} values
 * @param {string} [own] The wiki's palette.
 * @return {*}
 */
export function resolve( values, own ) {
	if ( !values || typeof values !== 'object' ) {
		return values;
	}
	const palette = values[ 'ui.palette' ];
	if ( typeof palette !== 'string' || !Object.prototype.hasOwnProperty.call( RETIRED, palette ) ) {
		return values;
	}
	const [ replaced, alternate ] = RETIRED[ palette ];
	const out = Object.assign( {}, values, { 'ui.palette': replaced === own ? '' : replaced } );
	if ( !values[ 'ui.vision' ] || values[ 'ui.vision' ] === 'none' ) {
		out[ 'ui.vision' ] = alternate;
	}
	return out;
}

/**
 * The keys that choose a stylesheet. Their choices are the wiki's, which the page lists;
 * an empty value, or none, is the wiki's own.
 *
 * @type {Object<string, string>}
 */
export const SHEETS = {
	'ui.palette': 'palettes',
	'ui.typeset': 'typesets'
};

/** The browser storage entry, one per host. */
export const STORAGE_KEY = 'ts-appearance';

/**
 * Where the stored choices came from: `browser`, made here while not logged in, or
 * `account`, a signed-in reader's account mirrored here. Only choices made in the
 * browser are carried to an account at login (ADR 0085 §6).
 */
export const SOURCE_KEY = 'ts-appearance-source';

const CLASSES = [ 'skin-theme-clientpref-os', 'skin-theme-clientpref-day', 'skin-theme-clientpref-night' ];

/**
 * Only the keys and values this build knows, and of the palette and type set only those
 * the wiki offers: a stored entry is the reader's own data, read back from a store
 * anything on the origin could have written.
 *
 * @param {*} values
 * @param {Object<string, string[]>} [sheets] The palettes and type sets a reader may
 *  choose here, by key; without them, none are kept.
 * @return {Object<string, string>}
 */
export function clean( values, sheets ) {
	const out = {};
	if ( !values || typeof values !== 'object' ) {
		return out;
	}
	for ( const key of Object.keys( CHOICES ) ) {
		if ( typeof values[ key ] === 'string' && CHOICES[ key ].includes( values[ key ] ) ) {
			out[ key ] = values[ key ];
		}
	}
	for ( const key of Object.keys( SHEETS ) ) {
		const allowed = sheets && sheets[ key ];
		const v = values[ key ];
		if ( typeof v === 'string' && Array.isArray( allowed ) && allowed.includes( v ) ) {
			out[ key ] = v;
		}
	}
	return out;
}

/**
 * @typedef {Object} Theme
 * @property {string} palette The wiki's palette.
 * @property {string} typeset The wiki's type set.
 * @property {Object<string, string>} palettes The stylesheet of each, by name.
 * @property {Object<string, string>} typesets The stylesheet of each, by name.
 */

/**
 * The page's theme block: the stylesheet of each palette and type set a reader may
 * choose, and the wiki's own.
 *
 * @param {Document} doc
 * @return {Theme|null}
 */
export function readTheme( doc ) {
	const block = doc.getElementById( 'ts-theme' );
	try {
		const t = JSON.parse( ( block && block.textContent ) || 'null' );
		return t && typeof t === 'object' && t.palettes && t.typesets ? t : null;
	} catch ( e ) {
		return null;
	}
}

/**
 * The palettes and type sets a reader may choose besides the wiki's own, by key.
 *
 * @param {Theme|null} theme What readTheme gives.
 * @return {Object<string, string[]>}
 */
export function sheetChoices( theme ) {
	const out = {};
	for ( const [ key, list ] of Object.entries( SHEETS ) ) {
		const own = theme && ( key === 'ui.palette' ? theme.palette : theme.typeset );
		out[ key ] = theme ? Object.keys( theme[ list ] ).filter( ( n ) => n !== own ) : [];
	}
	return out;
}

/**
 * Points the page's palette and type-set stylesheets at the reader's choices, or the
 * wiki's own where they have none. A link already pointing there is left alone, so the
 * request already made is the one used.
 *
 * @param {Document} doc
 * @param {Object<string, string>} values
 * @param {Theme|null} theme What readTheme gives.
 */
export function applySheets( doc, values, theme ) {
	if ( !theme ) {
		return;
	}
	for ( const [ key, list ] of Object.entries( SHEETS ) ) {
		const link = doc.getElementById( key === 'ui.palette' ? 'ts-palette' : 'ts-typeset' );
		const own = key === 'ui.palette' ? theme.palette : theme.typeset;
		const wanted = values && values[ key ];
		const name = Object.prototype.hasOwnProperty.call( theme[ list ], wanted ) ? wanted : own;
		const href = theme[ list ][ name ];
		if ( link && href && link.getAttribute( 'href' ) !== href ) {
			link.setAttribute( 'href', href );
		}
	}
}

/**
 * Writes the reader's choices on the root element: `data-mode`, `data-contrast`,
 * `data-text-size` and `data-vision`, and MediaWiki's `skin-theme-clientpref-*` class
 * beside them.
 *
 * @param {HTMLElement} root
 * @param {Object<string, string>} values
 */
export function apply( root, values ) {
	const v = clean( values );
	const mode = v[ 'ui.mode' ] || 'auto';
	if ( mode === 'auto' ) {
		root.removeAttribute( 'data-mode' );
	} else {
		root.setAttribute( 'data-mode', mode );
	}
	root.classList.remove( ...CLASSES );
	root.classList.add( {
		dark: 'skin-theme-clientpref-night',
		light: 'skin-theme-clientpref-day'
	}[ mode ] || 'skin-theme-clientpref-os' );
	if ( v[ 'ui.contrast' ] === 'more' ) {
		root.setAttribute( 'data-contrast', 'more' );
	} else {
		root.removeAttribute( 'data-contrast' );
	}
	if ( v[ 'ui.text-size' ] ) {
		root.setAttribute( 'data-text-size', v[ 'ui.text-size' ] );
	} else {
		root.removeAttribute( 'data-text-size' );
	}
	if ( v[ 'ui.vision' ] && v[ 'ui.vision' ] !== 'none' ) {
		root.setAttribute( 'data-vision', v[ 'ui.vision' ] );
	} else {
		root.removeAttribute( 'data-vision' );
	}
}

/**
 * The stored choices, or null where storage is absent or refused. A retired palette
 * reads as what replaced it.
 *
 * @param {Storage|undefined} storage
 * @param {Object<string, string[]>} [sheets] The palettes and type sets kept, by key.
 * @param {string} [own] The wiki's palette.
 * @return {Object<string, string>|null}
 */
export function read( storage, sheets, own ) {
	try {
		return clean( resolve( JSON.parse( storage.getItem( STORAGE_KEY ) || 'null' ), own ), sheets );
	} catch ( e ) {
		return null;
	}
}

/**
 * Keeps the choices, and where they came from; storage that is absent or refused keeps
 * nothing.
 *
 * @param {Storage|undefined} storage
 * @param {Object<string, string>} values
 * @param {string} [from] `browser` (the default) or `account`
 * @param {Object<string, string[]>} [sheets] The palettes and type sets kept, by key.
 */
export function write( storage, values, from, sheets ) {
	try {
		storage.setItem( STORAGE_KEY, JSON.stringify( clean( values, sheets ) ) );
		storage.setItem( SOURCE_KEY, from === 'account' ? 'account' : 'browser' );
	} catch ( e ) {
		// Nothing can be kept; the page follows the device.
	}
}

/**
 * Where the stored choices came from; `browser` for an entry older than the marker.
 *
 * @param {Storage|undefined} storage
 * @return {string|null} `browser`, `account`, or null where storage is unreadable
 */
export function source( storage ) {
	try {
		return storage.getItem( SOURCE_KEY ) === 'account' ? 'account' : 'browser';
	} catch ( e ) {
		return null;
	}
}
