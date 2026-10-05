// Editing with JavaScript (ADR 0034 §4, §5): a signed-in viewer edits an entity's terms
// in place, the server's rendering of the region replaces the editor, and that rendering
// matches the region of a fresh page. Requests alternate between the two web replicas.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs Alice in through the site's own form.
 *
 * @param {Object} page A Playwright page
 */
async function logIn( page ) {
	await page.goto( '/wiki/Special:UserLogin?returnto=Item:Q2' );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item:Q2$/ );
}

/**
 * The terms region as the server renders it now, for the signed-in viewer.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string>}
 */
async function serverTerms( page ) {
	const r = await page.request.get( '/w/index.php?title=Item:Q2&action=render&region=terms&uselang=en' );
	expect( r.status() ).toBe( 200 );
	return ( await r.text() ).trim();
}

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'a reader without an account sees no edit buttons', async ( { page } ) => {
		await page.goto( '/wiki/Item:Q2' );
		await expect( page.locator( '[data-ts-edit]' ) ).toHaveCount( 0 );
	} );

	test( 'the term editor saves label, description and aliases in place', async ( { page } ) => {
		await logIn( page );
		const edit = page.getByRole( 'button', { name: 'Edit the label, description and aliases' } );
		await expect( edit ).toBeVisible();
		await edit.click();
		const label = page.getByLabel( 'Label', { exact: true } );
		await expect( label ).toHaveValue( 'number' );
		await expect( label ).toBeFocused();
		const axe = await new AxeBuilder( { page } ).include( '.ts-terms-editor' ).analyze();
		expect( axe.violations.map( ( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.html ).join( ' | ' ) }` ) ).toEqual( [] );

		await label.fill( 'number (edited)' );
		await page.getByLabel( 'Description', { exact: true } ).fill( 'a mathematical object' );
		await page.getByLabel( 'Also known as', { exact: true } ).fill( 'numeral\nfigure' );
		await page.getByRole( 'button', { name: 'Save' } ).click();

		await expect( page.locator( '.ts-terms-editor' ) ).toHaveCount( 0 );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'number (edited)' );
		const region = page.locator( '[data-region="terms"]' );
		await expect( region ).toContainText( 'a mathematical object' );
		await expect( region ).toContainText( 'numeral · figure' );
		await expect( edit ).toBeFocused();

		// The swapped-in region is the server's own rendering.
		const swapped = await region.evaluate( ( el ) => el.outerHTML.replace( /\s(hidden|disabled)(="")?(?=[\s>])/g, '' ) );
		const fresh = ( await serverTerms( page ) ).replace( /\s(hidden|disabled)(="")?(?=[\s>])/g, '' );
		expect( swapped ).toBe( fresh );

		// And it stays saved.
		await page.reload();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'number (edited)' );
		await expect( page ).toHaveTitle( 'number (edited) (Q2) – librarybase' );

		// Put the label back for the other tests; Escape closes an editor unsaved.
		await edit.click();
		await page.getByLabel( 'Label', { exact: true } ).press( 'Escape' );
		await expect( page.locator( '.ts-terms-editor' ) ).toHaveCount( 0 );
		await edit.click();
		await page.getByLabel( 'Label', { exact: true } ).fill( 'number' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'number' );
	} );

	test( 'an edit conflict saves nothing and offers the current version', async ( { page } ) => {
		await logIn( page );
		await page.getByRole( 'button', { name: 'Edit the label, description and aliases' } ).click();
		await expect( page.getByLabel( 'Label', { exact: true } ) ).toHaveValue( 'number' );
		// Someone else saves first.
		const tokens = await ( await page.request.get( '/w/api.php?action=query&meta=tokens&format=json&formatversion=2' ) ).json();
		const other = await page.request.post( '/w/api.php', { form: {
			action: 'wbsetdescription', id: 'Q2', language: 'en', value: 'changed elsewhere',
			token: tokens.query.tokens.csrftoken, format: 'json', formatversion: '2'
		} } );
		expect( ( await other.json() ).success ).toBe( 1 );
		await page.getByLabel( 'Description', { exact: true } ).fill( 'mine' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'Someone changed this entity after you began editing' ) ).toBeVisible();
		await page.getByRole( 'button', { name: 'Start again from the current version' } ).click();
		await expect( page.getByLabel( 'Description', { exact: true } ) ).toHaveValue( 'changed elsewhere' );
		await page.getByRole( 'button', { name: 'Cancel' } ).click();
	} );
} );
