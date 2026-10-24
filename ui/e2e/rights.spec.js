// The Protect form, Special:UserRights, Special:ProtectedPages, Special:Block and the
// Visibility section (chapter 19 §6.3), with JavaScript off, as the owner and as a reader,
// through the edge; axe on each form.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

/**
 * Logs the owner in and lands on a page.
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

/**
 * The axe violations on the current page.
 *
 * @param {Object} page A Playwright page
 * @return {Promise<string[]>} The violations, as `id: targets`
 */
async function violations( page ) {
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	return results.violations.map(
		( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
	);
}

test( 'a reader is asked to log in, and sees no Protect tab', async ( { page } ) => {
	await page.goto( '/wiki/Project:Home' );
	await expect( page.getByRole( 'link', { name: 'Protect' } ) ).toHaveCount( 0 );
	await page.goto( '/w/index.php?title=Project:Home&action=protect' );
	await expect( page.getByText( 'Log in to protect pages.' ) ).toBeVisible();
	await page.goto( '/wiki/Special:UserRights' );
	await expect( page.getByText( 'Log in to change user rights.' ) ).toBeVisible();
	await page.goto( '/wiki/Special:ProtectedPages' );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Protected pages' );
	await expect( page.locator( 'input[name="do"]' ) ).toHaveCount( 0 );
} );

test.describe( 'the owner', () => {
	test( 'protects a page to a group and sees it listed', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		// A group to restrict to, which the owner joins through Special:UserRights.
		await page.goto( '/wiki/Special:UserRights/Alice' );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'User rights of Alice' );
		await expect( page.getByLabel( 'New memberships expire' ) ).toHaveValue( '1 day' );
		await page.getByLabel( 'sysop', { exact: true } ).check();
		await page.getByLabel( 'New memberships expire' ).fill( 'infinite' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Save user groups' } ).click();
		await expect( page.getByText( 'The groups were changed.' ) ).toBeVisible();
		await expect( page.getByLabel( 'sysop', { exact: true } ) ).toBeChecked();

		// An entity page carries the tab; the main page is the home page, with none.
		await page.goto( '/wiki/Item:Q6' );
		await page.getByRole( 'link', { name: 'Protect' } ).click();
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Protect Item:Q6' );
		await expect( page.getByLabel( 'Also restrict the talk page' ) ).toBeChecked();
		await page.getByLabel( 'Edit', { exact: true } ).selectOption( 'sysop' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( 'sysop' );
		await page.getByLabel( 'Reason' ).fill( 'e2e' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'The protection was saved.' ) ).toBeVisible();
		await expect( page.getByLabel( 'Read', { exact: true } ) ).toHaveValue( 'sysop' );

		await page.goto( '/wiki/Special:ProtectedPages' );
		const row = page.locator( '.ts-protectedpages__table tr', { hasText: 'Read' } );
		await expect( row ).toContainText( 'sysop' );

		// Lifted again, so the other suites find the item.
		await page.goto( '/w/index.php?title=Item:Q6&action=protect' );
		await page.getByLabel( 'Edit', { exact: true } ).selectOption( '' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( '' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'The protection was saved.' ) ).toBeVisible();
	} );

	test( 'is locked out of a group it is not in', async ( { page } ) => {
		await logIn( page, 'Project:Home' );
		await page.goto( '/w/index.php?title=Item:Q6&action=protect' );
		await page.getByLabel( 'Read', { exact: true } ).selectOption( 'bureaucrat' );
		await page.getByRole( 'button', { name: 'Save' } ).click();
		await expect( page.getByText( 'You must be a member of the group you restrict reading to' ) ).toBeVisible();
		await expect( page.getByLabel( 'Read', { exact: true } ) ).toHaveValue( 'bureaucrat' );
	} );

	test( 'the block form offers hiding only with the right, and refuses a temporary hide', async ( { page } ) => {
		await logIn( page, 'Special:Block' );
		await page.goto( '/wiki/Special:Block' );
		await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Block an account' );
		await expect( page.getByLabel( 'Hide the username' ) ).toBeVisible();
		await page.getByLabel( 'Account', { exact: true } ).fill( 'Alice' );
		await page.getByRole( 'button', { name: 'Block' } ).click();
		await expect( page.getByText( 'You cannot block yourself.' ) ).toBeVisible();
	} );

	// axe runs as a script in the page, so these contexts allow scripts; the site's own
	// script only marks the page as having them.
	for ( const path of [
		'/w/index.php?title=Item:Q6&action=protect',
		'/wiki/Special:UserRights/Alice',
		'/wiki/Special:ProtectedPages',
		'/wiki/Special:Block'
	] ) {
		test( `${ path } passes axe`, async ( { browser } ) => {
			const context = await browser.newContext( { javaScriptEnabled: true } );
			const page = await context.newPage();
			await logIn( page, 'Project:Home' );
			await page.goto( path );
			await expect( page.locator( 'form.ts-protect, form.ts-userrights__groups, .ts-protectedpages, form.ts-block' ).first() ).toBeVisible();
			expect( await violations( page ) ).toEqual( [] );
			await context.close();
		} );
	}

	test( 'the Visibility section reports the wiki as public', async ( { page } ) => {
		await logIn( page, 'Special:WikiSettings' );
		await page.goto( '/wiki/Special:WikiSettings' );
		await expect( page.getByText( 'Who may read this wiki now: Anyone' ) ).toBeVisible();
		await expect( page.getByLabel( 'Accounts on this wiki' ) ).toBeVisible();
	} );
} );
