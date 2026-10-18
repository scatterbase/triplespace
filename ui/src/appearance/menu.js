// The Appearance menu with JavaScript (ADR 0085 §6; 0086 §5; 19 §2.7). A change applies at
// once: a colour mode, contrast or text size on <html>, a palette or type set by pointing
// the page's stylesheet link at it.
// Not logged in, it is kept in this browser; signed in, it is written to the account
// through action=options and mirrored into the browser, marked as the account's, so it
// stays after logout. At a signed-in page, choices the reader made in this browser while
// not logged in are written to an account that holds none of its own, so a choice made
// before signing up survives it; otherwise the account's win. A key the reader changes
// while that check is under way keeps the reader's value.

/**
 * @param {string} url
 * @param {Object} [init] What `fetch` takes beside the URL.
 * @return {Promise<any>}
 */
async function json( url, init ) {
	const r = await fetch( url, Object.assign( { credentials: 'same-origin' }, init ) );
	if ( !r.ok ) {
		throw new Error( `${ url }: ${ r.status }` );
	}
	return r.json();
}

/**
 * Writes choices to the account.
 *
 * @param {Object<string, string>} values
 * @return {Promise<void>}
 */
export async function save( values ) {
	const t = await json( '/w/api.php?action=query&meta=tokens&type=csrf&format=json&formatversion=2' );
	const body = new URLSearchParams( {
		action: 'options',
		format: 'json',
		formatversion: '2',
		// A key without a value resets it: an empty palette or type set is the wiki's own.
		change: Object.entries( values ).map( ( [ k, v ] ) => ( v === '' ? k : `${ k }=${ v }` ) ).join( '|' ),
		token: t.query.tokens.csrftoken
	} );
	await json( '/w/api.php', { method: 'POST', body } );
}

/**
 * Whether the account holds any appearance key of its own.
 *
 * @param {string[]} keys
 * @return {Promise<boolean>}
 */
async function accountHolds( keys ) {
	const p = await json( '/w/rest.php/triplespace/v0/account/preferences' );
	return keys.some( ( k ) => p.preferences && p.preferences[ k ] && p.preferences[ k ].own );
}

/**
 * Wires the menu, if the page has one and the head script ran.
 *
 * @param {Document} doc
 * @param {Window} win
 */
export function initAppearance( doc, win ) {
	const block = doc.getElementById( 'ts-appearance-data' );
	const form = doc.querySelector( '.ts-appearance__form' );
	const app = win.tsAppearance;
	if ( !block || !form || !app ) {
		return;
	}
	let data;
	try {
		data = JSON.parse( block.textContent || '' );
	} catch ( e ) {
		return;
	}
	let storage;
	try {
		storage = win.localStorage;
	} catch ( e ) {
		storage = undefined;
	}
	const root = doc.documentElement;
	const theme = app.readTheme( doc );
	/** The palettes and type sets a reader may choose here, besides the wiki's own. */
	const sheets = data.choices || app.sheetChoices( theme );
	const selects = Array.from( form.querySelectorAll( 'select[data-key]' ) );
	const show = ( values ) => {
		for ( const s of selects ) {
			const v = values[ s.dataset.key ];
			if ( typeof v === 'string' ) {
				s.value = v;
			} else if ( s.dataset.key in sheets ) {
				s.value = '';
			}
		}
	};
	const kept = data.signedIn ? {} : app.read( storage, sheets ) || {};
	let current = Object.assign( {}, data.values || {}, kept );
	/** Keys the reader has changed on this page. */
	const touched = new Set();
	const mirror = () => app.write( storage, current, 'account', sheets );
	show( current );

	if ( data.signedIn ) {
		const stored = app.read( storage, sheets ) || {};
		if ( app.source( storage ) === 'browser' && Object.keys( stored ).length ) {
			accountHolds( data.keys ).then( ( holds ) => {
				const carried = {};
				if ( !holds ) {
					for ( const [ k, v ] of Object.entries( stored ) ) {
						if ( !touched.has( k ) && current[ k ] !== v ) {
							carried[ k ] = v;
						}
					}
				}
				current = Object.assign( {}, current, carried );
				app.apply( root, current );
				app.applySheets( doc, current, theme );
				show( current );
				mirror();
				return Object.keys( carried ).length ? save( carried ) : undefined;
			} ).catch( () => {
				// Left marked as the browser's, so the next page checks again.
			} );
		} else {
			mirror();
		}
	}

	form.addEventListener( 'change', ( e ) => {
		const target = /** @type {HTMLSelectElement} */ ( e.target );
		const key = target && target.dataset && target.dataset.key;
		if ( !key ) {
			return;
		}
		touched.add( key );
		current = Object.assign( {}, current, { [ key ]: target.value } );
		app.apply( root, current );
		if ( key in sheets ) {
			app.applySheets( doc, current, theme );
		}
		if ( data.signedIn ) {
			mirror();
			save( { [ key ]: target.value } ).catch( () => {
				// The form still works: Apply posts it to the web tier.
				form.classList.add( 'ts-appearance__form--unsaved' );
			} );
		} else {
			app.write( storage, current, 'browser', sheets );
		}
	} );
	form.addEventListener( 'submit', ( e ) => {
		if ( !data.signedIn ) {
			e.preventDefault();
		}
	} );
}
