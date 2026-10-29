import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import * as core from './core.js';
import { initAppearance } from './menu.js';

const DEFAULTS = { 'ui.mode': 'auto', 'ui.contrast': 'auto', 'ui.text-size': 'medium' };

/**
 * A page as the server draws it: the menu's radio buttons for mode, contrast and text size,
 * its selects for the rest, and the data block.
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
	const choices = Object.assign( {}, core.CHOICES, { 'ui.palette': [ '', 'encyclopedia' ] } );
	const icons = [ 'ui.mode', 'ui.contrast', 'ui.text-size' ];
	const selects = Object.entries( choices ).map( ( [ k, cs ] ) => ( icons.includes( k ) ?
		`<fieldset>${ cs.map( ( c ) => `<label><input type="radio" name="${ k }" value="${ c }" data-key="${ k }">${ c }</label>` ).join( '' ) }</fieldset>` :
		`<select data-key="${ k }">${ cs.map( ( c ) => `<option value="${ c }">${ c }</option>` ).join( '' ) }</select>` ) ).join( '' );
	document.head.innerHTML = `<script type="application/json" id="ts-theme">${ JSON.stringify( {
		palette: 'parchment', typeset: 'default26',
		palettes: { parchment: '/p-parchment.css', encyclopedia: '/p-encyclopedia.css' },
		typesets: { default26: '/t-default26.css' }
	} ) }</script><link id="ts-palette" href="/p-parchment.css"><link id="ts-typeset" href="/t-default26.css">`;
	document.body.innerHTML = `<form class="ts-appearance__form">${ selects }</form>` +
		`<script type="application/json" id="ts-appearance-data">${ JSON.stringify( {
			signedIn,
			keys: Object.keys( choices ),
			choices: { 'ui.palette': [ 'encyclopedia' ], 'ui.typeset': [] },
			values: signedIn ? Object.assign( { 'ui.palette': '' }, DEFAULTS ) : null
		} ) }</script>`;
	window.tsAppearance = core;
}

/**
 * Chooses a value in the menu, as a reader does: clicks its button, or picks it.
 *
 * @param {string} key
 * @param {string} value
 */
function choose( key, value ) {
	const radio = document.querySelector( `input[data-key="${ key }"][value="${ value }"]` );
	if ( radio ) {
		// As a click does, though a click on the button already chosen changes nothing;
		// the race below is of a reader choosing a value the account shows.
		radio.checked = true;
		radio.dispatchEvent( new Event( 'change', { bubbles: true } ) );
		return;
	}
	const s = document.querySelector( `select[data-key="${ key }"]` );
	s.value = value;
	s.dispatchEvent( new Event( 'change', { bubbles: true } ) );
}

/**
 * The button chosen in a group.
 *
 * @param {string} key
 * @return {string|undefined}
 */
function chosen( key ) {
	const r = document.querySelector( `input[data-key="${ key }"]:checked` );
	return r ? r.value : undefined;
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
		expect( chosen( 'ui.contrast' ) ).toBe( 'more' );
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
		expect( chosen( 'ui.mode' ) ).toBe( 'dark' );
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

	it( 'shows the stored choices and applies a colour-vision alternate', () => {
		core.write( localStorage, { 'ui.text-size': 'large' }, 'browser' );
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( false );
		initAppearance( document, window );
		expect( chosen( 'ui.text-size' ) ).toBe( 'large' );
		choose( 'ui.vision', 'rg' );
		expect( document.documentElement.getAttribute( 'data-vision' ) ).toBe( 'rg' );
		expect( core.read( localStorage ) ).toEqual( { 'ui.text-size': 'large', 'ui.vision': 'rg' } );
		choose( 'ui.vision', 'none' );
		expect( document.documentElement.hasAttribute( 'data-vision' ) ).toBe( false );
	} );

	it( 'reads a retired palette kept in the browser as the alternate', () => {
		localStorage.setItem( core.STORAGE_KEY, JSON.stringify( { 'ui.palette': 'parchment-rg' } ) );
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( false );
		initAppearance( document, window );
		expect( document.querySelector( 'select[data-key="ui.vision"]' ).value ).toBe( 'rg' );
		expect( document.querySelector( 'select[data-key="ui.palette"]' ).value ).toBe( '' );
	} );

	it( 'swaps the palette\'s stylesheet at once, and keeps the choice in the browser', () => {
		const a = api( false );
		vi.stubGlobal( 'fetch', a.fetch );
		page( false );
		initAppearance( document, window );
		choose( 'ui.palette', 'encyclopedia' );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/p-encyclopedia.css' );
		expect( core.read( localStorage, { 'ui.palette': [ 'encyclopedia' ] } ) ).toEqual( { 'ui.palette': 'encyclopedia' } );
		choose( 'ui.palette', '' );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/p-parchment.css' );
		expect( core.read( localStorage, { 'ui.palette': [ 'encyclopedia' ] } ) ).toEqual( {} );
	} );

	it( 'resets the account\'s palette when the reader goes back to the wiki\'s', async () => {
		core.write( localStorage, DEFAULTS, 'account' );
		const a = api( true );
		vi.stubGlobal( 'fetch', a.fetch );
		page( true );
		initAppearance( document, window );
		choose( 'ui.palette', 'encyclopedia' );
		await flush();
		choose( 'ui.palette', '' );
		await flush();
		await flush();
		expect( a.saved ).toEqual( [ 'ui.palette=encyclopedia', 'ui.palette' ] );
	} );
} );
