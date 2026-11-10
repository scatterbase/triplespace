// The bell's panel (ADR 0021 A20; ADR 0034 §2; 19 §6.2). Without JavaScript the bell is a
// link to Special:Notifications; with it, the bell opens the inbox as a panel over the
// page. The panel is the inbox page's own list, fetched from the page and drawn in
// place, so there is one rendering of an item. Opening it is looking, as opening the
// page is: the bell's mark goes. Mark as read and Mark all as read post in place and the
// panel is drawn again. A modified click, or a fetch that fails, follows the link.

/**
 * The inbox's list and the bell as the inbox page now draws them.
 *
 * @param {Document} doc
 * @param {string} html
 * @return {{ list: Element|null, bell: Element|null }}
 */
function parse( doc, html ) {
	const page = new DOMParser().parseFromString( html, 'text/html' );
	const list = page.querySelector( '.ts-notifications' );
	return {
		list: list ? doc.importNode( list, true ) : null,
		bell: page.querySelector( '.ts-bell' )
	};
}

/**
 * Wires the bell, if the page has one.
 *
 * @param {Document} doc
 * @param {Window} win
 */
export function initBell( doc, win ) {
	const bell = /** @type {HTMLAnchorElement|null} */ ( doc.querySelector( 'a.ts-bell' ) );
	const wrap = bell && bell.parentElement;
	if ( !bell || !wrap || !wrap.classList.contains( 'ts-bell-menu' ) ) {
		return;
	}
	const href = bell.getAttribute( 'href' ) || '';
	const panel = doc.createElement( 'div' );
	panel.className = 'ts-menu__items ts-bell__panel';
	panel.id = 'ts-bell-panel';
	panel.setAttribute( 'role', 'dialog' );
	panel.setAttribute( 'aria-label', bell.dataset.panelLabel || '' );
	panel.tabIndex = -1;
	panel.hidden = true;
	const body = doc.createElement( 'div' );
	body.className = 'ts-bell__body';
	const foot = doc.createElement( 'p' );
	foot.className = 'ts-bell__foot';
	const all = doc.createElement( 'a' );
	all.href = href;
	all.textContent = bell.dataset.all || '';
	foot.appendChild( all );
	panel.append( body, foot );
	wrap.appendChild( panel );
	bell.setAttribute( 'role', 'button' );
	bell.setAttribute( 'aria-haspopup', 'dialog' );
	bell.setAttribute( 'aria-controls', panel.id );
	bell.setAttribute( 'aria-expanded', 'false' );

	/**
	 * The bell as the fetched page draws it: its count, its label, and no mark, since
	 * the fetch was a look.
	 *
	 * @param {Element|null} fresh
	 */
	function sync( fresh ) {
		bell.classList.remove( 'ts-bell--unseen' );
		if ( !fresh ) {
			return;
		}
		const count = fresh.querySelector( '.ts-bell__count' );
		const old = bell.querySelector( '.ts-bell__count' );
		if ( old ) {
			old.remove();
		}
		if ( count ) {
			bell.appendChild( doc.importNode( count, true ) );
		}
		for ( const name of [ 'aria-label', 'title' ] ) {
			const v = fresh.getAttribute( name );
			if ( v ) {
				bell.setAttribute( name, v );
			}
		}
	}

	/**
	 * Draws a response of the inbox page into the panel.
	 *
	 * @param {Response} r
	 * @return {Promise<void>}
	 */
	async function draw( r ) {
		if ( !r.ok ) {
			throw new Error( String( r.status ) );
		}
		const got = parse( doc, await r.text() );
		if ( !got.list ) {
			// Not the inbox: the session ended, and the page asks to log in.
			throw new Error( 'no list' );
		}
		body.replaceChildren( got.list );
		sync( got.bell );
	}

	function close() {
		if ( panel.hidden ) {
			return;
		}
		panel.hidden = true;
		bell.setAttribute( 'aria-expanded', 'false' );
	}

	async function open() {
		panel.hidden = false;
		bell.setAttribute( 'aria-expanded', 'true' );
		const status = doc.createElement( 'p' );
		status.className = 'ts-bell__status';
		status.setAttribute( 'role', 'status' );
		status.textContent = bell.dataset.loading || '';
		body.replaceChildren( status );
		panel.focus();
		try {
			await draw( await win.fetch( href, { credentials: 'same-origin' } ) );
		} catch ( e ) {
			win.location.assign( href );
		}
	}

	bell.addEventListener( 'click', ( e ) => {
		if ( e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey ) {
			return;
		}
		e.preventDefault();
		if ( panel.hidden ) {
			open();
		} else {
			close();
		}
	} );
	// A link with the button role answers Space as a button does.
	bell.addEventListener( 'keydown', ( e ) => {
		if ( e.key === ' ' ) {
			e.preventDefault();
			bell.click();
		}
	} );
	// Mark as read, in place.
	panel.addEventListener( 'submit', async ( e ) => {
		const form = /** @type {HTMLFormElement} */ ( e.target );
		if ( !form.classList || !form.classList.contains( 'ts-notif__mark' ) ) {
			return;
		}
		e.preventDefault();
		const fields = new URLSearchParams();
		for ( const el of Array.from( form.elements ) ) {
			const input = /** @type {HTMLInputElement} */ ( el );
			// The panel shows everything, whichever view the form would return to.
			if ( input.name && input.name !== 'unread' ) {
				fields.append( input.name, input.value );
			}
		}
		try {
			// The answer is the redirect back to the inbox, which fetch follows.
			await draw( await win.fetch( form.getAttribute( 'action' ) || href, {
				method: 'POST',
				credentials: 'same-origin',
				body: fields
			} ) );
			panel.focus();
		} catch ( err ) {
			win.location.assign( href );
		}
	} );
	doc.addEventListener( 'keydown', ( e ) => {
		if ( e.key === 'Escape' && !panel.hidden ) {
			close();
			bell.focus();
		}
	} );
	doc.addEventListener( 'click', ( e ) => {
		if ( !panel.hidden && !wrap.contains( /** @type {Node} */ ( e.target ) ) ) {
			close();
		}
	} );
}
