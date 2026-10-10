// Creating entities without JavaScript (ADR 0047 §5; architecture 21 §3.1): the New menu's
// Special:NewItem and Special:NewProperty, posted through either replica, with prefilled
// statements, and the forms checked by axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form, returning to `title`.
 *
 * @param {Object} page A Playwright page
 * @param {string} title The page to return to
 */
async function logIn( page, title ) {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ title }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in' } ).click();
	await page.waitForURL( `**/wiki/${ title }` );
}

/**
 * axe's violations within the form, as text.
 *
 * @param {Object} browser A Playwright browser
 * @param {Object} page The page whose cookies and URL to use
 * @return {Promise<string[]>}
 */
async function axe( browser, page ) {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	await context.addCookies( await page.context().cookies() );
	const p = await context.newPage();
	await p.goto( page.url() );
	const results = await new AxeBuilder( { page: p } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	await context.close();
	return results.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }` );
}

test( 'a reader is asked to log in first', async ( { page } ) => {
	await page.goto( '/wiki/Special:NewItem' );
	await expect( page.getByText( 'Log in to create items and properties.' ) ).toBeVisible();
	await expect( page.locator( 'input[name="token"]' ) ).toHaveCount( 0 );
} );

test( 'the New menu creates an item, with prefilled statements', async ( { page, browser } ) => {
	await logIn( page, 'Project:Home' );
	await page.locator( '.ts-menu summary', { hasText: 'New' } ).click();
	await page.getByRole( 'link', { name: 'New item' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Special:NewItem$/ );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Create a new item' );

	// A link with statements, as a resolver's "create an item with this key" makes.
	await page.goto( '/wiki/Special:NewItem?statement=P7:0000%200004%201234%205678&statement=P2:Q2&statement=P4:1952' );
	await expect( page.getByLabel( 'ISNI (P7): 0000 0004 1234 5678' ) ).toBeChecked();
	await expect( page.getByLabel( 'instance of (P2): Q2' ) ).toBeChecked();
	await expect( page.getByText( 'values of type Point in time cannot be filled in here yet' ) ).toBeVisible();
	expect( await axe( browser, page ) ).toEqual( [] );

	await page.getByLabel( 'Label', { exact: true } ).fill( 'Created in e2e' );
	await page.getByLabel( 'Description', { exact: true } ).fill( 'an item made by the form' );
	await page.getByLabel( 'Also known as', { exact: true } ).fill( 'e2e item|form item' );
	await page.getByLabel( 'instance of (P2): Q2' ).uncheck();
	await page.getByRole( 'button', { name: 'Create item' } ).click();

	await expect( page ).toHaveURL( /\/wiki\/Item:Q\d+$/ );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Created in e2e' );
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'an item made by the form' );
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'e2e item · form item' );
	// The unticked statement was left out; the identifier is on its tab.
	await expect( page.locator( '#content' ) ).toContainText( 'No statements yet.' );
	await page.goto( `${ page.url() }?tab=identifiers` );
	await expect( page.locator( '#content' ) ).toContainText( '0000 0004 1234 5678' );
} );

test( 'a refused form keeps what was typed', async ( { page } ) => {
	await logIn( page, 'Special:NewItem' );
	await page.getByLabel( 'Language', { exact: true } ).fill( 'not a code' );
	await page.getByLabel( 'Label', { exact: true } ).fill( 'Kept' );
	await page.getByRole( 'button', { name: 'Create item' } ).click();
	await expect( page.getByText( '"not a code" is not a language code.' ) ).toBeVisible();
	await expect( page.getByLabel( 'Label', { exact: true } ) ).toHaveValue( 'Kept' );
} );

test( 'the owner creates a property with its data type', async ( { page, browser } ) => {
	await logIn( page, 'Special:NewProperty' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Create a new property' );
	expect( await axe( browser, page ) ).toEqual( [] );
	await page.getByLabel( 'Label', { exact: true } ).fill( 'e2e identifier' );
	await page.getByLabel( 'Data type' ).selectOption( 'external-id' );
	await page.getByRole( 'button', { name: 'Create property' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Property:P\d+$/ );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'e2e identifier' );
	await expect( page.locator( '#content' ) ).toContainText( 'External identifier' );
} );
