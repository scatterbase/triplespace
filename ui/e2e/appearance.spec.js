// The Appearance menu (ADR 0085 §6; chapter 19 §2.7): for a reader who is not logged in,
// a choice applies at once and is kept in the browser, and the head script applies it
// before the first paint of the next page, on either web replica, with no cookie; without
// JavaScript the menu says the page follows the device. Signed in, the choices are the
// account's and the server writes them on <html>. Then the settings and preferences
// pages, signed in, with axe.
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
	await page.getByRole( 'button', { name: 'Log in' } ).click();
}

test( 'without JavaScript the menu says the page follows the device', async ( { page } ) => {
	await page.goto( '/wiki/Special:RecentChanges' );
	await page.locator( '.ts-appearance > summary' ).click();
	await expect( page.locator( '.ts-appearance__nojs' ) ).toBeVisible();
	await expect( page.locator( '.ts-appearance__controls' ) ).toBeHidden();
	await expect( page.locator( 'html' ) ).not.toHaveAttribute( 'data-mode', /.+/ );
} );

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'a reader\'s choice is kept in the browser and applied before paint', async ( { page, context } ) => {
		await page.goto( '/wiki/Special:RecentChanges' );
		await page.locator( '.ts-appearance > summary' ).click();
		await expect( page.locator( '.ts-appearance__nojs' ) ).toBeHidden();
		await page.locator( 'select[data-key="ui.mode"]' ).selectOption( 'dark' );
		await page.locator( 'select[data-key="ui.text-size"]' ).selectOption( 'large' );
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
		await expect( page.locator( 'select[data-key="ui.mode"]' ) ).toHaveValue( 'dark' );
		expect( ( await context.cookies() ).filter( ( c ) => c.name !== 'triplespace_session' ) ).toEqual( [] );
	} );

	test( 'signed in, a choice is the account\'s and the server writes it', async ( { page } ) => {
		await logIn( page, 'Special:Preferences' );
		await page.goto( '/wiki/Special:RecentChanges' );
		await page.locator( '.ts-appearance > summary' ).click();
		await expect( page.locator( '.ts-appearance__apply' ) ).toBeHidden();
		const saved = page.waitForResponse( ( r ) => r.url().endsWith( '/w/api.php' ) && r.request().method() === 'POST' );
		await page.locator( 'select[data-key="ui.contrast"]' ).selectOption( 'more' );
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
