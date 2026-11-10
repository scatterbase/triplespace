<template>
	<div class="ts-sheet__panel ts-sheet__lookup">
		<span>{{ label }}</span>
		<EntityLookup
			:key="at"
			:api="api"
			:lang="lang"
			type="item"
			:label="label"
			@pick="( id ) => emit( 'pick', id )"
		/>
	</div>
</template>

<script setup lang="ts">
// The search beside a grid for the cell that has the focus (ADR 0106 §6, A7): an item for
// a cell of items, or the unit of a quantity. What is picked is written into the cell as
// its ID, which is what the table's text holds.
import EntityLookup from '../edit/EntityLookup.vue';
import type { Api } from '../edit/api';

defineProps<{
	api: Api;
	lang: string;
	/** What the search is for, in words. */
	label: string;
	/** The cell, so that another cell starts a new search. */
	at: string;
}>();

const emit = defineEmits<{
	pick: [ id: string ];
}>();
</script>
