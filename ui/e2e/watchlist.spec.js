// Watching (ADR 0020 §3; chapter 19 §6.2), with JavaScript off: the star on an entity, the
// Watch for… menu with a span and Notify me, Special:EditWatchlist; a new account's topic
// on the entity's talk page reaching Alice's watchlist as a change she has not seen and her
// bell as a `watch` notification; Mark all changes as seen; removing the watch. Then axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form.
 *
 * @param {Object} page A Playwright page
 * @param {string} returnto The title to come back to
 */
async function logIn( page, returnto ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ encodeURIComponent( returnto ) }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
}

/**
 * axe's WCAG 2.1 AA violations on the page as it stands.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string[]>}
 */
async function violations( page ) {
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	return results.violations.map(
		( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
	);
}

test.describe.configure( { mode: 'serial' } );

test( 'Alice watches an entity for a week, with notifications', async ( { page } ) => {
	await logIn( page, 'Item:Q6' );
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await page.getByRole( 'button', { name: 'Watch', exact: true } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q6$/ );
	await expect( page.getByRole( 'button', { name: 'Unwatch', exact: true } ) ).toBeVisible();

	await page.locator( '.ts-watch__more summary' ).click();
	await page.getByLabel( 'For a week' ).check();
	await page.getByLabel( 'Notify me of changes' ).check();
	await page.getByRole( 'button', { name: 'Save', exact: true } ).click();
	await expect( page.locator( '.ts-watch__until' ) ).toContainText( 'Watched until' );

	await page.goto( '/wiki/Special:EditWatchlist' );
	const row = page.locator( 'tr' ).filter( { hasText: 'Item:Q6' } );
	await expect( row ).toContainText( 'Yes' );
	await expect( row ).not.toContainText( 'Permanently' );
} );

test( 'a topic on the talk page reaches the watchlist and the bell', async ( { browser, page } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const other = await context.newPage();
	await other.goto( '/wiki/Special:CreateAccount?returnto=Item_talk:Q6' );
	await other.getByLabel( 'Username' ).fill( 'Watch Poster' );
	await other.getByLabel( 'Password', { exact: true } ).fill( 'watch-poster-password-1' );
	await other.getByLabel( 'Confirm password' ).fill( 'watch-poster-password-1' );
	await other.getByRole( 'button', { name: 'Create your account' } ).click();
	await other.waitForURL( '**/wiki/Item_talk:Q6', { timeout: 60000 } );
	await other.getByRole( 'link', { name: 'Add topic' } ).click();
	await other.getByLabel( 'Subject' ).fill( 'Is six a city?' );
	await other.getByLabel( 'Description' ).fill( 'Asking for the watchers.' );
	await other.getByRole( 'button', { name: 'Add topic' } ).click();
	await expect( other ).toHaveURL( /\/wiki\/Item_talk:Q6#thread-\d+$/ );
	await context.close();

	await logIn( page, 'Special:Watchlist' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Watchlist' );
	const unseen = page.locator( '.ts-change--unseen' ).filter( { hasText: 'Is six a city?' } );
	await expect( unseen ).toHaveCount( 1 );
	await page.getByRole( 'button', { name: 'Mark all changes as seen' } ).click();
	await expect( page.locator( '.ts-change--unseen' ) ).toHaveCount( 0 );
	await expect( page.locator( '.ts-change' ).filter( { hasText: 'Is six a city?' } ) ).toHaveCount( 1 );

	await page.goto( '/wiki/Special:Notifications' );
	await expect( page.locator( '.ts-notif' ).filter( { hasText: 'Is six a city?' } ) )
		.toContainText( 'Watch Poster posted in “Is six a city?”, which you watch.' );
} );

test( 'the watchlist pages and the menu pass axe, and the watch is removed', async ( { browser } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await logIn( page, 'Item:Q6' );
	await page.locator( '.ts-watch__more summary' ).click();
	expect( await violations( page ) ).toEqual( [] );
	await page.goto( '/wiki/Special:Watchlist' );
	expect( await violations( page ) ).toEqual( [] );
	await page.goto( '/wiki/Special:EditWatchlist' );
	expect( await violations( page ) ).toEqual( [] );
	await page.getByLabel( 'Remove Item:Q6' ).check();
	await page.getByRole( 'button', { name: 'Remove selected' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Special:EditWatchlist$/ );
	await expect( page.locator( 'tr' ).filter( { hasText: 'Item:Q6' } ) ).toHaveCount( 0 );
	await context.close();
} );
