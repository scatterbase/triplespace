import { describe, it, expect, beforeEach } from 'vitest';
import { apply, applySheets, clean, read, readTheme, sheetChoices, write, STORAGE_KEY } from './core.js';

const THEME = {
	palette: 'parchment',
	typeset: 'default26',
	palettes: { parchment: '/ui/theme/p-parchment-1.css', 'parchment-ob': '/ui/theme/p-parchment-ob-2.css' },
	typesets: { default26: '/ui/theme/t-default26-3.css', opendyslexic: '/ui/theme/t-opendyslexic-4.css' }
};

describe( 'appearance', () => {
	beforeEach( () => {
		document.documentElement.className = 'client-js skin-theme-clientpref-os';
		for ( const a of [ 'data-mode', 'data-contrast', 'data-text-size' ] ) {
			document.documentElement.removeAttribute( a );
		}
		localStorage.clear();
	} );

	it( 'keeps only known keys and values', () => {
		expect( clean( { 'ui.mode': 'dark', 'ui.contrast': 'purple', x: 'y' } ) ).toEqual( { 'ui.mode': 'dark' } );
		expect( clean( null ) ).toEqual( {} );
		expect( clean( 'dark' ) ).toEqual( {} );
	} );

	it( 'writes attributes and the MediaWiki classes', () => {
		const root = document.documentElement;
		apply( root, { 'ui.mode': 'dark', 'ui.contrast': 'more', 'ui.text-size': 'large' } );
		expect( root.getAttribute( 'data-mode' ) ).toBe( 'dark' );
		expect( root.classList.contains( 'skin-theme-clientpref-night' ) ).toBe( true );
		expect( root.classList.contains( 'skin-theme-clientpref-os' ) ).toBe( false );
		expect( root.getAttribute( 'data-contrast' ) ).toBe( 'more' );
		expect( root.getAttribute( 'data-text-size' ) ).toBe( 'large' );
		apply( root, { 'ui.mode': 'auto' } );
		expect( root.hasAttribute( 'data-mode' ) ).toBe( false );
		expect( root.hasAttribute( 'data-contrast' ) ).toBe( false );
		expect( root.classList.contains( 'skin-theme-clientpref-os' ) ).toBe( true );
		expect( root.classList.contains( 'client-js' ) ).toBe( true );
	} );

	it( 'stores per host and survives refused storage', () => {
		write( localStorage, { 'ui.mode': 'light', bogus: 1 } );
		expect( JSON.parse( localStorage.getItem( STORAGE_KEY ) ) ).toEqual( { 'ui.mode': 'light' } );
		expect( read( localStorage ) ).toEqual( { 'ui.mode': 'light' } );
		const refusing = { getItem() {
			throw new Error( 'denied' );
		}, setItem() {
			throw new Error( 'denied' );
		} };
		expect( read( refusing ) ).toBe( null );
		expect( () => write( refusing, { 'ui.mode': 'dark' } ) ).not.toThrow();
		expect( read( undefined ) ).toBe( null );
	} );

	it( 'keeps a palette or type set only where the wiki offers it', () => {
		const sheets = sheetChoices( THEME );
		expect( sheets ).toEqual( { 'ui.palette': [ 'parchment-ob' ], 'ui.typeset': [ 'opendyslexic' ] } );
		expect( clean( { 'ui.palette': 'parchment-ob', 'ui.typeset': 'comic' }, sheets ) ).toEqual( { 'ui.palette': 'parchment-ob' } );
		expect( clean( { 'ui.palette': 'parchment-ob' } ) ).toEqual( {} );
		write( localStorage, { 'ui.palette': 'parchment-ob', 'ui.mode': 'dark' }, 'browser', sheets );
		expect( read( localStorage, { 'ui.palette': [] } ) ).toEqual( { 'ui.mode': 'dark' } );
		expect( read( localStorage, sheets ) ).toEqual( { 'ui.palette': 'parchment-ob', 'ui.mode': 'dark' } );
	} );

	it( 'points the stylesheet links at the chosen palette and type set', () => {
		document.head.innerHTML = `<script type="application/json" id="ts-theme">${ JSON.stringify( THEME ) }</script>` +
			'<link id="ts-palette" href="/ui/theme/p-parchment-1.css">' +
			'<link id="ts-typeset" href="/ui/theme/t-default26-3.css">';
		const theme = readTheme( document );
		expect( theme ).toEqual( THEME );
		applySheets( document, { 'ui.palette': 'parchment-ob', 'ui.typeset': 'opendyslexic' }, theme );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/ui/theme/p-parchment-ob-2.css' );
		expect( document.getElementById( 'ts-typeset' ).getAttribute( 'href' ) ).toBe( '/ui/theme/t-opendyslexic-4.css' );
		applySheets( document, { 'ui.palette': '', 'ui.typeset': 'nope' }, theme );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/ui/theme/p-parchment-1.css' );
		expect( document.getElementById( 'ts-typeset' ).getAttribute( 'href' ) ).toBe( '/ui/theme/t-default26-3.css' );
		document.getElementById( 'ts-theme' ).textContent = '{';
		expect( readTheme( document ) ).toBe( null );
		expect( () => applySheets( document, {}, null ) ).not.toThrow();
		document.head.innerHTML = '';
	} );
} );
