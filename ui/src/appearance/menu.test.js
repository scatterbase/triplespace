import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import * as core from './core.js';
import { initAppearance } from './menu.js';

const DEFAULTS = { 'ui.mode': 'auto', 'ui.contrast': 'auto', 'ui.text-size': 'medium' };

/**
 * A page as the server draws it: the menu's selects and the data block.
 *
 * @param {boolean} signedIn
 */
function page( signedIn ) {
	const root = document.documentElement;
	root.className = 'client-js skin-theme-clientpref-os';
	for ( const a of [ 'data-mode', 'data-contrast', 'data-text-size', 'data-appearance' ] ) {
		root.removeAttribute( a );
	}
	if ( signedIn ) {
		root.setAttribute( 'data-appearance', 'account' );
	}
	const selects = Object.entries( core.CHOICES ).map( ( [ k, cs ] ) => `<select data-key="${ k }">${ cs.map( ( c ) => `<option value="${ c }">${ c }</option>` ).join( '' ) }</select>` ).join( '' );
	document.body.innerHTML = `<form class="ts-appearance__form">${ selects }</form>` +
		`<script type="application/json" id="ts-appearance-data">${ JSON.stringify( {
			signedIn, keys: Object.keys( core.CHOICES ), values: signedIn ? DEFAULTS : null
		} ) }</script>`;
	window.tsAppearance = core;
}

/**
 * Chooses a value in the menu, as a reader does.
 *
 * @param {string} key
 * @param {string} value
 */
function choose( key, value ) {
	const s = document.querySelector( `select[data-key="${ key }"]` );
	s.value = value;
	s.dispatchEvent( new Event( 'change', { bubbles: true } ) );
}

/**
 * A fake API: tokens, action=options (recorded), and the preferences route, whose
 * answer the test releases when it chooses.
 *
 * @param {boolean} ownValues Whether the account holds appearance keys of its own
 * @return {Object}
 */
function api( ownValues ) {
	let release;
	const prefs = new Promise( ( r ) => {
		release = r;
	} );
	const saved = [];
	const fetch = vi.fn( async ( url, init ) => {
		const ok = ( body ) => ( { ok: true, json: async () => body } );
		if ( url.includes( 'meta=tokens' ) ) {
			return ok( { query: { tokens: { csrftoken: 't' } } } );
		}
		if ( url.endsWith( '/account/preferences' ) ) {
			await prefs;
			return ok( { preferences: { 'ui.mode': { own: ownValues }, 'ui.contrast': { own: false } } } );
		}
		saved.push( Object.fromEntries( new URLSearchParams( init.body ) ).change );
		return ok( { options: 'success' } );
	} );
	return { fetch, saved, release: () => release() };
}

const flush = () => new Promise( ( r ) => {
	setTimeout( r, 0 );
} );

describe( 'the Appearance menu', () => {
	beforeEach( () => localStorage.clear() );
	afterEach( () => vi.unstubAllGlobals() );

	it( 'does not carry the account\'s own mirrored values back to it (the CI race)', async () => {
		core.write( localStorage, DEFAULTS, 'account' );
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( true );
		initAppearance( document, window );
		choose( 'ui.contrast', 'more' );
		await flush();
		a.release();
		await flush();
		await flush();
		expect( document.documentElement.getAttribute( 'data-contrast' ) ).toBe( 'more' );
		expect( a.fetch.mock.calls.some( ( [ u ] ) => u.endsWith( '/account/preferences' ) ) ).toBe( false );
		expect( a.saved ).toEqual( [ 'ui.contrast=more' ] );
		expect( core.read( localStorage )[ 'ui.contrast' ] ).toBe( 'more' );
		expect( core.source( localStorage ) ).toBe( 'account' );
	} );

	it( 'carries choices made while not logged in to an account without its own', async () => {
		core.write( localStorage, { 'ui.mode': 'dark' }, 'browser' );
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( true );
		initAppearance( document, window );
		a.release();
		await flush();
		await flush();
		await flush();
		expect( document.documentElement.getAttribute( 'data-mode' ) ).toBe( 'dark' );
		expect( a.saved ).toEqual( [ 'ui.mode=dark' ] );
		expect( core.source( localStorage ) ).toBe( 'account' );
	} );

	it( 'keeps a change the reader makes while the check is under way', async () => {
		core.write( localStorage, { 'ui.mode': 'dark', 'ui.contrast': 'more' }, 'browser' );
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( true );
		initAppearance( document, window );
		choose( 'ui.contrast', 'auto' );
		a.release();
		await flush();
		await flush();
		await flush();
		const root = document.documentElement;
		expect( root.hasAttribute( 'data-contrast' ) ).toBe( false );
		expect( root.getAttribute( 'data-mode' ) ).toBe( 'dark' );
		expect( a.saved.sort() ).toEqual( [ 'ui.contrast=auto', 'ui.mode=dark' ] );
	} );

	it( 'lets an account with its own values win', async () => {
		core.write( localStorage, { 'ui.mode': 'dark' }, 'browser' );
		const a = api( true );
		vi.stubGlobal( 'fetch', a.fetch );
		page( true );
		initAppearance( document, window );
		a.release();
		await flush();
		await flush();
		expect( document.documentElement.hasAttribute( 'data-mode' ) ).toBe( false );
		expect( a.saved ).toEqual( [] );
		expect( core.read( localStorage )[ 'ui.mode' ] ).toBe( 'auto' );
		expect( core.source( localStorage ) ).toBe( 'account' );
	} );

	it( 'keeps a reader\'s choice in the browser when not logged in', () => {
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( false );
		initAppearance( document, window );
		choose( 'ui.mode', 'light' );
		expect( document.documentElement.getAttribute( 'data-mode' ) ).toBe( 'light' );
		expect( core.read( localStorage ) ).toEqual( { 'ui.mode': 'light' } );
		expect( core.source( localStorage ) ).toBe( 'browser' );
		expect( a.fetch ).not.toHaveBeenCalled();
	} );
} );
