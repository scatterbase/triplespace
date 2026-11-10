import { describe, it, expect, vi } from 'vitest';
import { initPrefTabs } from './tabs.js';

// 19 §2.6: with JavaScript, Special:Preferences shows one group at a time.
describe( 'initPrefTabs', () => {
	/**
	 * @param {string} hash
	 * @param {string} [saved] The section that carries a message.
	 * @return {{ doc: Document, win: any }}
	 */
	function page( hash, saved ) {
		const doc = document.implementation.createHTMLDocument( 't' );
		const ids = [ 'interface', 'appearance', 'history' ];
		doc.body.innerHTML = '<nav class="ts-tabs">' +
			ids.map( ( i ) => `<a class="ts-tabs__link" href="#${ i }">${ i }</a>` ).join( '' ) +
			'</nav><div class="ts-preferences">' +
			ids.map( ( i ) => `<section class="ts-settings__section" id="${ i }">${ i === saved ? '<div class="cdx-message">Saved</div>' : '' }<div id="pref-${ i }-key"></div></section>` ).join( '' ) +
			'<section class="ts-settings__section" id="restore"></section></div>';
		/** @type {Object<string, Function>} */
		const listeners = {};
		const win = {
			location: { hash },
			history: { pushState: vi.fn() },
			addEventListener: ( /** @type {string} */ n, /** @type {Function} */ f ) => {
				listeners[ n ] = f;
			},
			listeners
		};
		initPrefTabs( doc, win );
		return { doc, win };
	}

	/**
	 * @param {Document} doc
	 * @return {string[]}
	 */
	const shown = ( doc ) => Array.from( doc.querySelectorAll( '.ts-settings__section' ) )
		.filter( ( s ) => !( /** @type {HTMLElement} */ ( s ).hidden ) ).map( ( s ) => s.id );

	it( 'shows the first group, and always the restore form', () => {
		const { doc } = page( '' );
		expect( shown( doc ) ).toEqual( [ 'interface', 'restore' ] );
		expect( doc.querySelector( '[aria-current="page"]' )?.textContent ).toBe( 'interface' );
		expect( doc.querySelector( '.ts-preferences--panes' ) ).not.toBeNull();
	} );

	it( 'opens the group the address names, or the one that holds the row it names', () => {
		expect( shown( page( '#history' ).doc ) ).toEqual( [ 'history', 'restore' ] );
		expect( shown( page( '#pref-appearance-key' ).doc ) ).toEqual( [ 'appearance', 'restore' ] );
	} );

	it( 'opens the group a save answered', () => {
		expect( shown( page( '', 'appearance' ).doc ) ).toEqual( [ 'appearance', 'restore' ] );
	} );

	it( 'switches on a tab, names the pane in the address, and follows the back button', () => {
		const { doc, win } = page( '' );
		const tab = /** @type {HTMLElement} */ ( doc.querySelector( 'a[href="#history"]' ) );
		const e = new MouseEvent( 'click', { bubbles: true, cancelable: true } );
		tab.dispatchEvent( e );
		expect( e.defaultPrevented ).toBe( true );
		expect( shown( doc ) ).toEqual( [ 'history', 'restore' ] );
		expect( win.history.pushState ).toHaveBeenCalledWith( null, '', '#history' );
		win.location.hash = '#interface';
		win.listeners.popstate();
		expect( shown( doc ) ).toEqual( [ 'interface', 'restore' ] );
	} );
} );
