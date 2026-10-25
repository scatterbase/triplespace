// Talk pages and threads (ADR 0019 §8; chapter 19 §6.1), with JavaScript off: starting a
// thread, replying with a status, and the signature an editor types by habit removed
// without comment (chapter 14 §1.6); then axe on both pages.
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

test( 'an empty talk page reads without JavaScript and offers no form to a reader', async ( { page } ) => {
	const response = await page.goto( '/wiki/Item_talk:Q5' );
	expect( response.status() ).toBe( 200 );
	await expect( page.getByRole( 'heading', { level: 1 } ) ).toHaveText( 'Item talk:Q5' );
	await expect( page.getByText( 'There are no discussions on this page yet.' ) ).toBeVisible();
	await expect( page.getByRole( 'link', { name: 'Discussion' } ) ).toHaveAttribute( 'aria-current', 'page' );
	await expect( page.getByRole( 'link', { name: 'Edit', exact: true } ) ).toHaveCount( 0 );
	await expect( page.getByRole( 'link', { name: 'Add topic' } ) ).toHaveCount( 0 );
} );

test( 'a thread is started, replied to and resolved from the site', async ( { page } ) => {
	await logIn( page, 'Item talk:Q5' );
	await page.getByRole( 'link', { name: 'Add topic' } ).click();
	await page.getByLabel( 'Subject' ).fill( 'Is five a city?' );
	await page.getByLabel( 'Description' ).fill( 'The label says so, the sources do not. --~~~~' );
	await expect( page.getByText( 'Posts on this wiki use Markdown.' ) ).toBeVisible();
	await page.getByRole( 'button', { name: 'Add topic' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Item_talk:Q5#thread-\d+$/ );
	const thread = page.locator( '.ts-thread' ).filter( { hasText: 'Is five a city?' } );
	await expect( thread.locator( '.ts-post' ).first() ).toContainText( 'The label says so, the sources do not.' );
	await expect( thread ).not.toContainText( '~~~~' );
	// No status yet: the thread reads as an ordinary section.
	await expect( thread.locator( '.ts-status' ) ).toHaveCount( 0 );

	await thread.getByRole( 'link', { name: 'Reply' } ).first().click();
	await expect( page.getByLabel( 'Reply to Alice' ) ).toBeVisible();
	await page.getByLabel( 'Reply to Alice' ).fill( 'Checked: it is not. ~~~~' );
	await page.getByLabel( 'Status' ).selectOption( 'resolved' );
	await page.getByRole( 'button', { name: 'Reply' } ).click();
	await expect( page ).toHaveURL( /\/wiki\/Thread:.*#post-\d+$/ );
	await expect( page.locator( '.ts-thread-page__state .ts-status' ) ).toHaveText( 'Resolved' );
	await expect( page.locator( '.ts-post' ).nth( 1 ) ).toContainText( 'Checked: it is not.' );
	await expect( page.locator( '.ts-post' ).nth( 1 ) ).not.toContainText( '~~~~' );

	// A post that is only a signature is refused, and nothing is lost.
	await page.getByLabel( 'Reply to this thread' ).fill( '~~~~' );
	await page.getByRole( 'button', { name: 'Reply' } ).click();
	await expect( page.getByText( 'Your post has no text.' ) ).toBeVisible();

	// Back on the talk page, the resolved thread is collapsed behind its summary.
	await page.getByRole( 'link', { name: 'Item talk:Q5' } ).click();
	const closed = page.locator( '.ts-thread' ).filter( { hasText: 'Is five a city?' } );
	await expect( closed.locator( '.ts-thread__head .ts-status' ) ).toHaveText( 'Resolved' );
	await expect( closed.getByText( 'Checked: it is not.' ) ).toBeHidden();
	await closed.locator( 'summary' ).click();
	await expect( closed.getByText( 'Checked: it is not.' ) ).toBeVisible();
} );

test( 'the talk page and the thread page pass axe', async ( { browser } ) => {
	const context = await browser.newContext( { javaScriptEnabled: true } );
	const page = await context.newPage();
	await logIn( page, 'Item talk:Q5' );
	await page.goto( '/wiki/Item_talk:Q5?action=addtopic' );
	expect( await violations( page ) ).toEqual( [] );
	await page.locator( '.ts-thread__subject a' ).first().click();
	await expect( page.locator( 'form#reply' ) ).toBeVisible();
	expect( await violations( page ) ).toEqual( [] );
	await context.close();
} );
