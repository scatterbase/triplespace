// Special:Preferences' tabs as panes (19 §2.6). Without JavaScript the tabs are links to
// the sections of one long page; with it, one section is in view at a time: the one the
// address names, else the one a save just answered (it carries the message), else the
// first. A tab's address is still `#{group}`, so a pane can be linked to and the back
// button returns to it. **Restore all defaults** stays under whichever pane is open.

/**
 * @param {Document} doc
 * @param {Window} win
 */
export function initPrefTabs( doc, win ) {
	const root = doc.querySelector( '.ts-preferences' );
	if ( !root ) {
		return;
	}
	/** @type {Map<string, { pane: HTMLElement, tab: HTMLElement }>} */
	const panes = new Map();
	for ( const tab of Array.from( doc.querySelectorAll( '.ts-tabs__link[href^="#"]' ) ) ) {
		const id = ( tab.getAttribute( 'href' ) || '' ).slice( 1 );
		const pane = id && doc.getElementById( id );
		if ( pane && root.contains( pane ) ) {
			panes.set( id, { pane, tab: /** @type {HTMLElement} */ ( tab ) } );
		}
	}
	if ( panes.size < 2 ) {
		return;
	}
	root.classList.add( 'ts-preferences--panes' );

	/**
	 * @param {string} id
	 */
	function show( id ) {
		for ( const [ key, { pane, tab } ] of panes ) {
			pane.hidden = key !== id;
			if ( key === id ) {
				tab.setAttribute( 'aria-current', 'page' );
			} else {
				tab.removeAttribute( 'aria-current' );
			}
		}
	}

	/**
	 * The pane the address names, if it names one.
	 *
	 * @return {string|undefined}
	 */
	function named() {
		const id = decodeURIComponent( ( win.location.hash || '' ).slice( 1 ) );
		if ( panes.has( id ) ) {
			return id;
		}
		// A row's anchor (`#pref-{key}`) opens the pane that holds it.
		const inside = id && doc.getElementById( id );
		for ( const [ key, { pane } ] of panes ) {
			if ( inside && pane.contains( inside ) ) {
				return key;
			}
		}
		return undefined;
	}

	let first;
	let answered;
	for ( const [ key, { pane } ] of panes ) {
		first = first || key;
		if ( !answered && pane.querySelector( '.cdx-message' ) ) {
			answered = key;
		}
	}
	show( named() || answered || /** @type {string} */ ( first ) );

	for ( const [ key, { tab } ] of panes ) {
		tab.addEventListener( 'click', ( e ) => {
			if ( e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey ) {
				return;
			}
			e.preventDefault();
			show( key );
			// The address names the pane, without the page jumping to it.
			win.history.pushState( null, '', `#${ key }` );
		} );
	}
	const back = () => {
		const id = named();
		if ( id ) {
			show( id );
		}
	};
	win.addEventListener( 'hashchange', back );
	win.addEventListener( 'popstate', back );
}
