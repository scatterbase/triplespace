// The reader's appearance on <html> (ADR 0085 §6; 0086 §4; 19 §5.9), shared by the head
// script, which applies a stored choice before the first paint, and the Appearance menu,
// which applies a change at once. The keys and their choices are preferences.toml's; the
// server's twin is triplespace-ui's appearance.rs.

/** @type {Object<string, string[]>} */
export const CHOICES = {
	'ui.mode': [ 'auto', 'light', 'dark' ],
	'ui.contrast': [ 'auto', 'more' ],
	'ui.text-size': [ 'small', 'medium', 'large', 'x-large' ]
};

/** The browser storage entry, one per host. */
export const STORAGE_KEY = 'ts-appearance';

const CLASSES = [ 'skin-theme-clientpref-os', 'skin-theme-clientpref-day', 'skin-theme-clientpref-night' ];

/**
 * Only the keys and values this build knows: a stored entry is the reader's own data,
 * read back from a store anything on the origin could have written.
 *
 * @param {*} values
 * @return {Object<string, string>}
 */
export function clean( values ) {
	const out = {};
	if ( !values || typeof values !== 'object' ) {
		return out;
	}
	for ( const key of Object.keys( CHOICES ) ) {
		if ( typeof values[ key ] === 'string' && CHOICES[ key ].includes( values[ key ] ) ) {
			out[ key ] = values[ key ];
		}
	}
	return out;
}

/**
 * Writes the reader's choices on the root element: `data-mode`, `data-contrast` and
 * `data-text-size`, and MediaWiki's `skin-theme-clientpref-*` class beside them.
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
}

/**
 * The stored choices, or null where storage is absent or refused.
 *
 * @param {Storage|undefined} storage
 * @return {Object<string, string>|null}
 */
export function read( storage ) {
	try {
		return clean( JSON.parse( storage.getItem( STORAGE_KEY ) || 'null' ) );
	} catch ( e ) {
		return null;
	}
}

/**
 * Keeps the choices; storage that is absent or refused keeps nothing.
 *
 * @param {Storage|undefined} storage
 * @param {Object<string, string>} values
 */
export function write( storage, values ) {
	try {
		storage.setItem( STORAGE_KEY, JSON.stringify( clean( values ) ) );
	} catch ( e ) {
		// Nothing can be kept; the page follows the device.
	}
}
