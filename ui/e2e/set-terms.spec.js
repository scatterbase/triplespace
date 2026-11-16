// Wikibase's term forms without JavaScript (ADR 0047 §5; architecture 21 §3.1):
// Special:SetLabel, SetDescription, SetAliases and SetLabelDescriptionAliases, through the
// real API, on an item made here so no other spec's entity changes; and Special:NewPages
// and Special:Providers, which read the same instance.
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
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await page.waitForURL( `**/wiki/${ title }` );
}

/**
 * axe's violations on the page, as text.
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
	await page.goto( '/wiki/Special:SetLabel/Q6/en' );
	await expect( page.getByText( 'Log in to change an entity' ) ).toBeVisible();
	await expect( page.locator( 'input[name="token"]' ) ).toHaveCount( 0 );
} );

test( 'the term forms change an item, one term or all three at once', async ( { page, browser } ) => {
	await logIn( page, 'Special:NewItem' );
	await page.getByLabel( 'Label', { exact: true } ).fill( 'Terms in e2e' );
	await page.getByRole( 'button', { name: 'Create item' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q\d+$/ );
	const id = page.url().match( /Item:(Q\d+)$/ )[ 1 ];

	// No entity yet: the form asks which.
	await page.goto( '/wiki/Special:SetLabel' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Set a label' );
	await page.getByLabel( 'Entity ID' ).fill( id );
	await page.getByRole( 'button', { name: 'Continue' } ).click();
	// The current label, filled in.
	await expect( page.getByLabel( 'Label', { exact: true } ) ).toHaveValue( 'Terms in e2e' );
	expect( await axe( browser, page ) ).toEqual( [] );
	await page.getByLabel( 'Label', { exact: true } ).fill( 'Terms renamed' );
	await page.getByRole( 'button', { name: 'Save' } ).click();
	await page.waitForURL( `**/wiki/Item:${ id }` );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Terms renamed' );

	// Wikibase's address with a value to prefill.
	await page.goto( `/wiki/Special:SetDescription/${ id }/en?value=set%20by%20address` );
	await expect( page.getByLabel( 'Description', { exact: true } ) ).toHaveValue( 'set by address' );
	await page.getByRole( 'button', { name: 'Save' } ).click();
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'set by address' );

	await page.goto( `/wiki/Special:SetAliases/${ id }/en` );
	await page.getByLabel( 'Aliases', { exact: true } ).fill( 'one|two' );
	await page.getByRole( 'button', { name: 'Save' } ).click();
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'one · two' );

	// All three in one edit; an empty alias field clears them.
	await page.goto( `/wiki/Special:SetLabelDescriptionAliases/${ id }/en` );
	await expect( page.getByLabel( 'Aliases', { exact: true } ) ).toHaveValue( 'one|two' );
	await page.getByLabel( 'Label', { exact: true } ).fill( 'Terms all at once' );
	await page.getByLabel( 'Description', { exact: true } ).fill( 'three terms, one revision' );
	await page.getByLabel( 'Aliases', { exact: true } ).fill( '' );
	await page.getByRole( 'button', { name: 'Save' } ).click();
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Terms all at once' );
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'three terms, one revision' );
	await expect( page.locator( '[data-region="terms"]' ) ).not.toContainText( 'one · two' );

	// The item is among the new pages.
	await page.goto( '/wiki/Special:NewPages' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'New pages' );
	await expect( page.locator( '#content' ) ).toContainText( id );
} );

test( 'a keyed entity is not offered a label, and an unknown ID is said to be one', async ( { page } ) => {
	await logIn( page, 'Special:SetLabelDescriptionAliases' );
	await page.goto( '/wiki/Special:SetLabelDescriptionAliases/domain:e2e-terms.example/en' );
	await expect( page.getByLabel( 'Label', { exact: true } ) ).toHaveCount( 0 );
	await page.getByLabel( 'Description', { exact: true } ).fill( 'a domain described by the form' );
	await page.getByRole( 'button', { name: 'Save' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Domain:e2e-terms\.example$/ );
	await expect( page.locator( '[data-region="terms"]' ) ).toContainText( 'a domain described by the form' );

	const r = await page.goto( '/wiki/Special:SetLabel/Q999999/en' );
	expect( r.status() ).toBe( 404 );
	await expect( page.getByText( 'There is no entity Q999999' ) ).toBeVisible();
} );

test( 'providers list the registry', async ( { page } ) => {
	await page.goto( '/wiki/Special:Providers' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Providers' );
	await expect( page.locator( '#content' ) ).toContainText( 'mirror/wikidata' );
	await expect( page.locator( '#content' ) ).toContainText( 'IDs WDQ…, in the Item namespace' );
} );
