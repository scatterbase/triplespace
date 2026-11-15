// ADR 0034 §4: the suggesting search box loads when the header's box is first used.
import { describe, expect, it } from 'vitest';
import { initTypeahead } from './typeahead.js';

const page = ( block ) => {
	document.body.innerHTML = `<form class="ts-header__search" role="search">
		<input type="search" name="search" aria-label="Search">
		<button type="submit">Search</button>${ block }</form>`;
	return document.querySelector( 'input[type="search"]' );
};
const data = '<script type="application/json" id="ts-search-data">{"lang":"en","kinds":["item"],"pages":true,"messages":{}}</script>';

describe( 'the header search loader', () => {
	it( 'loads the component once, on first focus, and hands it the form and the data', async () => {
		const input = page( data );
		const mounted = [];
		let loads = 0;
		initTypeahead( document, () => {
			loads++;
			return Promise.resolve( {
				mount: ( form, d ) => mounted.push( [ form.className, d.lang ] )
			} );
		} );
		expect( loads ).toBe( 0 );
		input.dispatchEvent( new Event( 'focus' ) );
		input.dispatchEvent( new Event( 'input' ) );
		await Promise.resolve();
		expect( loads ).toBe( 1 );
		expect( mounted ).toEqual( [ [ 'ts-header__search', 'en' ] ] );
	} );

	it( 'tries again after a failed load, and does nothing without the data', async () => {
		const input = page( data );
		let loads = 0;
		initTypeahead( document, () => {
			loads++;
			return Promise.reject( new Error( 'offline' ) );
		} );
		input.dispatchEvent( new Event( 'focus' ) );
		await Promise.resolve();
		await Promise.resolve();
		input.dispatchEvent( new Event( 'input' ) );
		expect( loads ).toBe( 2 );

		const plain = page( '' );
		let asked = 0;
		initTypeahead( document, () => {
			asked++;
			return Promise.resolve( { mount() {} } );
		} );
		plain.dispatchEvent( new Event( 'focus' ) );
		expect( asked ).toBe( 0 );
	} );
} );
