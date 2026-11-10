import { describe, it, expect, vi } from 'vitest';
import { initBell } from './bell.js';

// ADR 0021 A20; 19 §6.2: with JavaScript the bell opens the inbox as a panel.
describe( 'initBell', () => {
	const BELL = '<span class="ts-menu ts-bell-menu"><a class="ts-bell ts-bell--unseen" href="/wiki/Special:Notifications" aria-label="Notifications, 2 unread" title="Notifications, 2 unread" data-panel-label="Notifications" data-all="All notifications" data-loading="Loading…"><span class="ts-bell__count" aria-hidden="true">2</span></a></span><p id="out">x</p>';

	/**
	 * The inbox page, with `n` unread items.
	 *
	 * @param {number} n
	 * @return {string}
	 */
	function inbox( n ) {
		const items = Array.from( { length: n }, ( _, i ) => `<li class="ts-notif ts-notif--unread"><p class="ts-notif__text">Item ${ i }</p><form class="ts-notif__mark" method="post" action="/w/index.php?title=Special:Notifications"><input type="hidden" name="wpEditToken" value="tok"><input type="hidden" name="markread" value="${ i }"><input type="hidden" name="unread" value="1"><button type="submit">Mark as read</button></form></li>` ).join( '' );
		const count = n ? `<span class="ts-bell__count" aria-hidden="true">${ n }</span>` : '';
		const label = n ? `Notifications, ${ n } unread` : 'Notifications';
		return `<html><body><a class="ts-bell" href="/wiki/Special:Notifications" aria-label="${ label }" title="${ label }">${ count }</a><div class="ts-notifications"><ul class="ts-notifications__list">${ items }</ul></div></body></html>`;
	}

	/**
	 * @param {Function} fetch
	 * @return {{ doc: Document, win: any, bell: HTMLElement }}
	 */
	function page( fetch ) {
		const win = { fetch, location: { assign: vi.fn() } };
		const doc = document.implementation.createHTMLDocument( 't' );
		doc.body.innerHTML = BELL;
		initBell( doc, win );
		return { doc, win, bell: /** @type {HTMLElement} */ ( doc.querySelector( '.ts-bell' ) ) };
	}

	/**
	 * @param {string} html
	 * @param {boolean} [ok]
	 * @return {Promise<any>}
	 */
	const answer = ( html, ok = true ) => Promise.resolve( {
		ok,
		status: ok ? 200 : 500,
		text: () => Promise.resolve( html )
	} );
	const settle = () => new Promise( ( r ) => {
		setTimeout( r, 0 );
	} );

	it( 'opens the inbox in a panel and clears the mark', async () => {
		const fetch = vi.fn( () => answer( inbox( 2 ) ) );
		const { doc, win, bell } = page( fetch );
		expect( bell.getAttribute( 'aria-expanded' ) ).toBe( 'false' );
		bell.dispatchEvent( new MouseEvent( 'click', { bubbles: true, cancelable: true } ) );
		await settle();
		const panel = /** @type {HTMLElement} */ ( doc.getElementById( 'ts-bell-panel' ) );
		expect( panel.hidden ).toBe( false );
		expect( bell.getAttribute( 'aria-expanded' ) ).toBe( 'true' );
		expect( fetch ).toHaveBeenCalledWith( '/wiki/Special:Notifications', expect.anything() );
		expect( panel.querySelectorAll( '.ts-notif' ).length ).toBe( 2 );
		expect( panel.querySelector( '.ts-bell__foot a' )?.textContent ).toBe( 'All notifications' );
		expect( bell.classList.contains( 'ts-bell--unseen' ) ).toBe( false );
		expect( win.location.assign ).not.toHaveBeenCalled();
	} );

	it( 'closes on Escape, on a second click and on a click outside', async () => {
		const { doc, bell } = page( () => answer( inbox( 1 ) ) );
		const panel = /** @type {HTMLElement} */ ( doc.getElementById( 'ts-bell-panel' ) );
		const click = ( /** @type {Element} */ el ) => el.dispatchEvent( new MouseEvent( 'click', { bubbles: true, cancelable: true } ) );
		click( bell );
		await settle();
		doc.body.dispatchEvent( new KeyboardEvent( 'keydown', { key: 'Escape', bubbles: true } ) );
		expect( panel.hidden ).toBe( true );
		click( bell );
		await settle();
		click( bell );
		expect( panel.hidden ).toBe( true );
		click( bell );
		await settle();
		click( /** @type {Element} */ ( doc.getElementById( 'out' ) ) );
		expect( panel.hidden ).toBe( true );
	} );

	it( 'marks as read in place and takes the new count', async () => {
		const fetch = vi.fn( ( /** @type {string} */ url, /** @type {any} */ init ) => answer( inbox( init && init.method === 'POST' ? 1 : 2 ) ) );
		const { doc, bell } = page( fetch );
		bell.dispatchEvent( new MouseEvent( 'click', { bubbles: true, cancelable: true } ) );
		await settle();
		const form = /** @type {HTMLFormElement} */ ( doc.querySelector( '#ts-bell-panel .ts-notif__mark' ) );
		form.dispatchEvent( new Event( 'submit', { bubbles: true, cancelable: true } ) );
		await settle();
		const post = fetch.mock.calls[ 1 ];
		expect( post[ 0 ] ).toBe( '/w/index.php?title=Special:Notifications' );
		expect( post[ 1 ].method ).toBe( 'POST' );
		expect( post[ 1 ].body.get( 'markread' ) ).toBe( '0' );
		expect( post[ 1 ].body.get( 'wpEditToken' ) ).toBe( 'tok' );
		expect( post[ 1 ].body.has( 'unread' ) ).toBe( false );
		expect( doc.querySelectorAll( '#ts-bell-panel .ts-notif' ).length ).toBe( 1 );
		expect( bell.querySelector( '.ts-bell__count' )?.textContent ).toBe( '1' );
		expect( bell.getAttribute( 'aria-label' ) ).toBe( 'Notifications, 1 unread' );
	} );

	it( 'follows the link on a modified click, and when the inbox cannot be fetched', async () => {
		const fetch = vi.fn( () => answer( '<html><body>Log in</body></html>' ) );
		const { win, bell } = page( fetch );
		const e = new MouseEvent( 'click', { bubbles: true, cancelable: true, ctrlKey: true } );
		bell.dispatchEvent( e );
		expect( e.defaultPrevented ).toBe( false );
		expect( fetch ).not.toHaveBeenCalled();
		bell.dispatchEvent( new MouseEvent( 'click', { bubbles: true, cancelable: true } ) );
		await settle();
		expect( win.location.assign ).toHaveBeenCalledWith( '/wiki/Special:Notifications' );
	} );
} );
