import { describe, it, expect, beforeEach } from 'vitest';
import { apply, clean, read, write, STORAGE_KEY } from './core.js';

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
} );
