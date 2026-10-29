// The Appearance menu (ADR 0085 §6; chapter 19 §2.7): for a reader who is not logged in,
// a choice applies at once and is kept in the browser, and the head script applies it
// before the first paint of the next page, on either web replica, with no cookie; without
// JavaScript the menu says the page follows the device. Signed in, the choices are the
// account's and the server writes them on <html>. The palette's and type set's
// stylesheets (ADR 0086 §5) follow the device without JavaScript, and the reader's own
// choice of palette and type set with it. Then the settings and preferences pages, signed
// in, with axe, and a page in each of the palette's variants, with axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form.
 *
 * @param {Object} page A Playwright page
 * @param {string} back The title to return to
 */
async function logIn( page, back ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ back }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

test( 'without JavaScript the menu says the page follows the device', async ( { page } ) => {
	await page.goto( '/wiki/Special:RecentChanges' );
	await page.locator( '.ts-appearance > summary' ).click();
	await expect( page.locator( '.ts-appearance__nojs' ) ).toBeVisible();
	await expect( page.locator( '.ts-appearance__controls' ) ).toBeHidden();
	await expect( page.locator( 'html' ) ).not.toHaveAttribute( 'data-mode', /.+/ );
} );

/**
 * A custom property's value on <html>, as the page computes it.
 *
 * @param {Object} page A Playwright page
 * @param {string} name The token, without `--`
 * @return {Promise<string>}
 */
function token( page, name ) {
	return page.evaluate( ( n ) => getComputedStyle( document.documentElement ).getPropertyValue( `--${ n }` ).trim(), name );
}

/**
 * Chooses one of the Appearance menu's icon buttons (colour mode, contrast, text size),
 * by clicking its label, as a reader does.
 *
 * @param {Object} page A Playwright page
 * @param {string} key
 * @param {string} value
 */
async function pick( page, key, value ) {
	await page.locator( `label:has( > input[data-key="${ key }"][value="${ value }"] )` ).click();
}

/**
 * The button chosen in one of the menu's groups.
 *
 * @param {Object} page A Playwright page
 * @param {string} key
 * @return {Object} A locator
 */
function chosen( page, key ) {
	return page.locator( `input[data-key="${ key }"]:checked` );
}

/**
 * The page's background, as the page computes it.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string>}
 */
function background( page ) {
	return page.evaluate( () => getComputedStyle( document.body ).backgroundColor );
}

test( 'without JavaScript the palette follows the device\'s dark mode and contrast', async ( { page } ) => {
	await page.goto( '/wiki/Special:RecentChanges' );
	// parchment's subtle panels: light, then walnut brown.
	expect( await background( page ) ).toBe( 'rgb(244, 242, 236)' );
	await page.emulateMedia( { colorScheme: 'dark' } );
	expect( await background( page ) ).toBe( 'rgb(39, 31, 25)' );
	const dark = await token( page, 'color-base' );
	await page.emulateMedia( { colorScheme: 'dark', contrast: 'more' } );
	expect( await token( page, 'color-base' ) ).not.toBe( dark );
	await page.emulateMedia( { colorScheme: 'light', contrast: 'no-preference' } );
	expect( await token( page, 'color-base' ) ).toBe( '#1D1C1A' );
	expect( await token( page, 'font-family-base' ) ).toContain( 'Source Sans 3' );
} );

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'a reader\'s choice is kept in the browser and applied before paint', async ( { page, context } ) => {
		await page.goto( '/wiki/Special:RecentChanges' );
		await page.locator( '.ts-appearance > summary' ).click();
		await expect( page.locator( '.ts-appearance__nojs' ) ).toBeHidden();
		await pick( page, 'ui.mode', 'dark' );
		await pick( page, 'ui.text-size', 'large' );
		const html = page.locator( 'html' );
		await expect( html ).toHaveAttribute( 'data-mode', 'dark' );
		await expect( html ).toHaveClass( /skin-theme-clientpref-night/ );
		await expect( html ).toHaveAttribute( 'data-text-size', 'large' );
		// The next page: the head script applies it, the page itself is unchanged.
		const response = await page.goto( '/wiki/Item:Q6' );
		expect( response.headers()[ 'cache-control' ] ).toMatch( /^public/ );
		expect( response.headers()[ 'set-cookie' ] ).toBeUndefined();
		await expect( html ).toHaveAttribute( 'data-mode', 'dark' );
		await expect( html ).toHaveAttribute( 'data-text-size', 'large' );
		await expect( chosen( page, 'ui.mode' ) ).toHaveValue( 'dark' );
		expect( ( await context.cookies() ).filter( ( c ) => c.name !== 'triplespace_session' ) ).toEqual( [] );
	} );

	test( 'a reader\'s palette, type set and text size change what the page wears', async ( { page } ) => {
		await page.goto( '/wiki/Special:RecentChanges' );
		await page.locator( '.ts-appearance > summary' ).click();
		const palette = page.locator( 'select[data-key="ui.palette"]' );
		await expect( palette ).toHaveValue( '' );
		await palette.selectOption( 'encyclopedia' );
		await expect( page.locator( '#ts-palette' ) ).toHaveAttribute( 'href', /\/ui\/theme\/p-encyclopedia-/ );
		await expect.poll( () => token( page, 'border-radius-base' ) ).toBe( '2px' );
		// The colour-vision alternate rides on whatever palette is worn (ADR 0094):
		// encyclopedia keys none, so it wears parchment's.
		await page.locator( 'select[data-key="ui.vision"]' ).selectOption( 'rg' );
		await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-vision', 'rg' );
		await expect.poll( () => token( page, 'color-destructive' ) ).toBe( '#A84300' );
		await page.locator( 'select[data-key="ui.typeset"]' ).selectOption( 'opendyslexic' );
		await expect.poll( () => token( page, 'font-family-base' ) ).toContain( 'OpenDyslexic' );
		await pick( page, 'ui.text-size', 'large' );
		expect( await token( page, 'font-size-medium' ) ).toBe( '1.125rem' );
		// The next page is the same public page; the head script points it at the
		// reader's stylesheets before the first paint.
		const response = await page.goto( '/wiki/Item:Q6' );
		expect( response.headers()[ 'cache-control' ] ).toMatch( /^public/ );
		await expect( page.locator( '#ts-palette' ) ).toHaveAttribute( 'href', /p-encyclopedia-/ );
		await expect( page.locator( '#ts-typeset' ) ).toHaveAttribute( 'href', /t-opendyslexic-/ );
		await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-vision', 'rg' );
		await expect.poll( () => token( page, 'color-destructive' ) ).toBe( '#A84300' );
		await expect( page.locator( 'select[data-key="ui.palette"]' ) ).toHaveValue( 'encyclopedia' );
		await expect( page.locator( 'select[data-key="ui.vision"]' ) ).toHaveValue( 'rg' );
		// In dark mode the alternate's dark colours apply.
		await page.locator( '.ts-appearance > summary' ).click();
		await pick( page, 'ui.mode', 'dark' );
		await expect.poll( () => token( page, 'color-error' ) ).toBe( '#F26100' );
		await page.locator( '.ts-appearance > summary' ).click();
		// Back to the wiki's own.
		await page.locator( '.ts-appearance > summary' ).click();
		await page.locator( 'select[data-key="ui.palette"]' ).selectOption( '' );
		await expect( page.locator( '#ts-palette' ) ).toHaveAttribute( 'href', /p-parchment-[0-9a-f]{16}\.css/ );
	} );

	test( 'a page in many scripts fetches the fallback faces of those scripts alone', async ( { page } ) => {
		// The Noto layer (ADR 0093): every face is declared, but the browser fetches one
		// only for characters on the page that the type set's own families lack.
		const fetched = [];
		page.on( 'request', ( r ) => {
			if ( r.url().endsWith( '.woff2' ) ) {
				fetched.push( r.url() );
			}
		} );
		await page.goto( '/wiki/Item:Q6' );
		await page.evaluate( async () => {
			const terms = [ [ 'ar', 'مرحبا' ], [ 'hi', 'नमस्ते' ], [ 'ka', 'გამარჯობა' ], [ 'ur', 'خوش آمدید' ] ];
			for ( const [ lang, text ] of terms ) {
				const p = document.createElement( 'p' );
				p.lang = lang;
				p.textContent = text;
				document.querySelector( 'main, body' ).append( p );
			}
			await document.fonts.ready;
		} );
		const has = ( file ) => fetched.some( ( u ) => u.includes( `/${ file }-` ) );
		await expect.poll( () => has( 'noto-nastaliq-urdu-arabic-400-normal' ) ).toBe( true );
		for ( const file of [ 'noto-sans-arabic-arabic', 'noto-sans-devanagari-devanagari', 'noto-sans-georgian-georgian' ] ) {
			expect( has( `${ file }-400-normal` ), file ).toBe( true );
		}
		// The page's own arrows (← → ↗), which Source Sans 3 lacks, take Noto Sans
		// Symbols; the scripts it does not hold take nothing.
		for ( const script of [ 'thai', 'ethiopic', 'hebrew', 'tamil' ] ) {
			expect( fetched.filter( ( u ) => u.includes( `-${ script }-` ) ), script ).toEqual( [] );
		}
		// Latin text is the type set's own: the layer's Latin faces are not fetched for it.
		expect( fetched.filter( ( u ) => u.includes( '/noto-sans-latin-' ) ) ).toEqual( [] );
	} );

	for ( const [ mode, contrast ] of [ [ 'light', 'auto' ], [ 'dark', 'auto' ], [ 'light', 'more' ], [ 'dark', 'more' ] ] ) {
		test( `a page in ${ mode } mode with ${ contrast } contrast passes axe`, async ( { page } ) => {
			await page.goto( '/wiki/Item:Q6' );
			await page.locator( '.ts-appearance > summary' ).click();
			await pick( page, 'ui.mode', mode );
			await pick( page, 'ui.contrast', contrast );
			await page.locator( '.ts-appearance > summary' ).click();
			await page.reload();
			await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-mode', mode );
			const results = await new AxeBuilder( { page } )
				.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
				.analyze();
			const found = results.violations.map(
				( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
			);
			expect( found ).toEqual( [] );
		} );
	}

	test( 'signed in, a choice is the account\'s and the server writes it', async ( { page } ) => {
		await logIn( page, 'Special:Preferences' );
		await page.goto( '/wiki/Special:RecentChanges' );
		await page.locator( '.ts-appearance > summary' ).click();
		await expect( page.locator( '.ts-appearance__apply' ) ).toBeHidden();
		const saved = page.waitForResponse( ( r ) => r.url().endsWith( '/w/api.php' ) && r.request().method() === 'POST' );
		await pick( page, 'ui.contrast', 'more' );
		expect( ( await saved ).status() ).toBe( 200 );
		await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-contrast', 'more' );
		// A fresh page: the server wrote it, and the head script left it alone.
		await page.goto( '/wiki/Special:RecentChanges' );
		await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-appearance', 'account' );
		await expect( page.locator( 'html' ) ).toHaveAttribute( 'data-contrast', 'more' );
	} );

	for ( const path of [ '/wiki/Special:WikiSettings', '/wiki/Special:Preferences' ] ) {
		test( `${ path } signed in passes axe`, async ( { page } ) => {
			await logIn( page, path.replace( '/wiki/', '' ) );
			await page.goto( path );
			await expect( page.locator( '.ts-settings__form' ).first() ).toBeVisible();
			const results = await new AxeBuilder( { page } )
				.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
				.analyze();
			const found = results.violations.map(
				( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
			);
			expect( found ).toEqual( [] );
		} );
	}
} );
